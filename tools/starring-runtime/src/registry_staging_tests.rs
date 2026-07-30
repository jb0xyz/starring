use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use automation_runtime_convergence::{FencingToken, ProcessInstanceId, RuntimeProcessIdentityV1};
use automation_runtime_registry::{
    ExactServingRouteV1, ServingSlotRegistryError, ServingSlotRegistryV1, SlotLifecycleV1,
};
use serde_json::json;

use super::{
    complete_staged_install_v2, compose_runtime_registry_bootstrap_v1,
    RuntimeRegistryBarrierBActivationErrorV2, RuntimeRegistryBarrierBActivationOutcomeV2,
    RuntimeRegistryBarrierBActivationV2, RuntimeRegistryBarrierBServingAuthorityV2,
    RuntimeRegistryEmergencyTriggerV2, RuntimeRegistryServingBindingV2,
    RuntimeRegistryStagedInstallOutcomeV2, RuntimeRegistryStagedInstallV2,
    RuntimeRegistryStagedRouteV2, RuntimeRegistryStagingErrorV2, RuntimeRegistryStagingPortV2,
};
use crate::GatewayResourceConfigV1;

const CONTENT_HASH: &str = "9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6";
const BINDING_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";

fn fence(value: u64) -> FencingToken {
    FencingToken::new(value).unwrap()
}

fn route(process_instance_id: &str) -> ExactServingRouteV1 {
    let identity: RuntimeProcessIdentityV1 = serde_json::from_value(json!({
        "target": {
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": CONTENT_HASH,
            "binding_revision": 1,
            "binding_fingerprint": BINDING_FINGERPRINT
        },
        "runtime_generation": 1,
        "process_instance_id": process_instance_id
    }))
    .unwrap();
    let ruleset = serde_json::from_value(json!({
        "guild_id": "42",
        "ruleset_key": "studyroom",
        "version": 1,
        "schema_version": 1,
        "definition": {
            "version": 1,
            "panels": [],
            "modals": [],
            "rules": []
        },
        "content_hash": CONTENT_HASH,
        "created_by": "9"
    }))
    .unwrap();

    ExactServingRouteV1::new(identity, ruleset, Default::default()).unwrap()
}

fn staging_port(
    process_instance_id: &str,
) -> (RuntimeRegistryStagingPortV2, ServingSlotRegistryV1) {
    let process_instance_id = ProcessInstanceId::parse(process_instance_id).unwrap();
    let bootstrap = compose_runtime_registry_bootstrap_v1(
        process_instance_id.clone(),
        GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let observation = bootstrap.observe_recovery_empty_projection_v2().unwrap();
    let registry = bootstrap.registry.clone();
    let binding = RuntimeRegistryServingBindingV2 {
        process_instance_id,
        registry: registry.clone(),
        initial_registry_observation_sequence: observation.observation_sequence(),
        initial_retained_slot_count: observation.retained_slot_count(),
        initial_retained_empty_tombstone_count: observation.retained_empty_tombstone_count(),
    };

    (binding.staging_port_v2(), registry)
}

fn emergency() -> (RuntimeRegistryEmergencyTriggerV2, Arc<AtomicUsize>) {
    let trips = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&trips);
    (
        RuntimeRegistryEmergencyTriggerV2::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }),
        trips,
    )
}

fn authority(install: RuntimeRegistryStagedInstallV2) -> RuntimeRegistryStagedRouteV2 {
    install.into_parts_v2().2
}

#[test]
fn staging_port_is_clone_send_sync_and_install_authority_is_send() {
    fn assert_port<T: Clone + Send + Sync>() {}
    fn assert_authority<T: Send>() {}

    assert_port::<RuntimeRegistryStagingPortV2>();
    assert_authority::<RuntimeRegistryStagedInstallV2>();
    assert_authority::<RuntimeRegistryStagedRouteV2>();
    assert_authority::<RuntimeRegistryBarrierBActivationV2>();
    assert_authority::<RuntimeRegistryBarrierBServingAuthorityV2>();
}

#[test]
fn staged_and_exact_replay_installs_return_exact_evidence_and_owned_authority() {
    let (port, registry) = staging_port("runtime-process:staging");
    let route = route("runtime-process:staging");
    let initial_registry_sequence = registry
        .recovery_observation_v2()
        .unwrap()
        .observation_sequence()
        .get();
    let (first_emergency, first_trips) = emergency();
    let first = port
        .install_staged_route_v2(route.clone(), fence(1), first_emergency)
        .unwrap();

    assert_eq!(
        first.outcome_v2(),
        RuntimeRegistryStagedInstallOutcomeV2::Installed
    );
    let first_evidence = first.evidence_v2();
    assert_eq!(first_evidence.identity_v2(), route.identity());
    assert_eq!(first_evidence.fencing_token_v2(), fence(1));
    assert_eq!(first_evidence.route.lifecycle, SlotLifecycleV1::Staged);
    assert_eq!(first_evidence.active_interactions_v2(), 0);
    assert!(first_evidence.registry_observation_sequence_v2().get() >= initial_registry_sequence);
    assert!(
        first_evidence.registry_observation_sequence_v2().get()
            >= first_evidence.slot_observation_sequence.get()
    );
    let first_route = first_evidence.route.clone();
    let first_admission_generation = first_evidence.admission_generation_v2();
    let first_slot_sequence = first_evidence.slot_observation_sequence;
    let first_registry_sequence = first_evidence.registry_observation_sequence_v2();
    let (_, first_evidence, mut first) = first.into_parts_v2();
    first.ensure_staged_v2().unwrap();
    assert_eq!(first.identity_v2(), route.identity());
    assert_eq!(first.fencing_token_v2(), fence(1));
    let first_guard_token = first.token.as_ref().unwrap().clone();
    assert_eq!(
        registry.route_witness(&first_guard_token).unwrap(),
        first_evidence.route.clone()
    );
    let first_atomic = registry
        .atomic_observation_v2(first_guard_token.key())
        .unwrap()
        .unwrap();
    assert_eq!(first_atomic.route.as_ref(), Some(&first_evidence.route));
    assert_eq!(
        first_atomic.active_interactions,
        first_evidence.active_interactions_v2()
    );
    assert_eq!(
        first_atomic.admission_generation,
        first_evidence.admission_generation_v2()
    );
    assert_eq!(
        first_atomic.observation_sequence,
        first_evidence.slot_observation_sequence
    );
    let first_token = first.token.take().unwrap();
    drop(first);
    drop(first_token);
    assert_eq!(first_trips.load(Ordering::SeqCst), 0);

    let (replay_emergency, replay_trips) = emergency();
    let replay = port
        .clone()
        .install_staged_route_v2(route, fence(1), replay_emergency)
        .unwrap();

    assert_eq!(
        replay.outcome_v2(),
        RuntimeRegistryStagedInstallOutcomeV2::ExactReplay
    );
    assert_eq!(&replay.evidence_v2().route, &first_route);
    assert_eq!(
        replay.evidence_v2().route_incarnation_v2(),
        first_route.incarnation
    );
    assert_eq!(
        replay.evidence_v2().admission_generation_v2(),
        first_admission_generation
    );
    assert!(replay.evidence_v2().slot_observation_sequence >= first_slot_sequence);
    assert!(replay.evidence_v2().registry_observation_sequence_v2() >= first_registry_sequence);
    let (_, _, replay) = replay.into_parts_v2();
    let replay_token = replay.token.as_ref().unwrap().clone();
    replay.ensure_staged_v2().unwrap();
    replay.remove_v2().unwrap();
    assert_eq!(
        registry.route_witness(&replay_token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(replay_trips.load(Ordering::SeqCst), 0);
}

#[test]
fn serving_and_draining_replays_are_rejected() {
    let (port, registry) = staging_port("runtime-process:lifecycle");
    let route = route("runtime-process:lifecycle");
    let receipt = registry
        .install(route.slot_key(), route.clone(), fence(1))
        .unwrap();
    registry.activate(&receipt.token, route.identity()).unwrap();
    let (serving_emergency, serving_trips) = emergency();

    assert!(matches!(
        port.install_staged_route_v2(route.clone(), fence(1), serving_emergency),
        Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)
    ));
    assert_eq!(serving_trips.load(Ordering::SeqCst), 0);

    registry.begin_drain(&receipt.token).unwrap();
    let (draining_emergency, draining_trips) = emergency();
    assert!(matches!(
        port.install_staged_route_v2(route, fence(1), draining_emergency),
        Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)
    ));
    assert_eq!(draining_trips.load(Ordering::SeqCst), 0);
}

#[test]
fn process_mismatch_is_rejected_before_registry_mutation() {
    let (port, registry) = staging_port("runtime-process:local");
    let (emergency, trips) = emergency();

    assert!(matches!(
        port.install_staged_route_v2(route("runtime-process:foreign"), fence(1), emergency),
        Err(RuntimeRegistryStagingErrorV2::ProcessMismatch)
    ));
    let observation = registry.recovery_observation_v2().unwrap();
    assert_eq!(observation.staged_route_count(), 0);
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn drop_cleanup_removes_exact_staged_route() {
    let (port, registry) = staging_port("runtime-process:drop");
    let (emergency, trips) = emergency();
    let staged = authority(
        port.install_staged_route_v2(route("runtime-process:drop"), fence(1), emergency)
            .unwrap(),
    );
    let token = staged.token.as_ref().unwrap().clone();

    drop(staged);

    assert_eq!(
        registry.route_witness(&token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn explicit_cleanup_removes_exact_staged_route() {
    let (port, registry) = staging_port("runtime-process:explicit");
    let (emergency, trips) = emergency();
    let staged = authority(
        port.install_staged_route_v2(route("runtime-process:explicit"), fence(1), emergency)
            .unwrap(),
    );
    let token = staged.token.as_ref().unwrap().clone();

    staged.remove_v2().unwrap();

    assert_eq!(
        registry.route_witness(&token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn authority_advance_replaces_the_exact_fence_and_remains_staged() {
    let (port, registry) = staging_port("runtime-process:advance");
    let (emergency, trips) = emergency();
    let mut staged = authority(
        port.install_staged_route_v2(route("runtime-process:advance"), fence(1), emergency)
            .unwrap(),
    );
    let previous = staged.token.as_ref().unwrap().clone();

    let evidence = staged.advance_authority_v2(fence(2)).unwrap();

    assert_eq!(staged.fencing_token_v2(), fence(2));
    assert_eq!(evidence.fencing_token_v2(), fence(2));
    assert_eq!(evidence.identity_v2(), staged.identity_v2());
    assert_eq!(evidence.active_interactions_v2(), 0);
    assert_eq!(
        evidence.route_incarnation_v2(),
        staged
            .registry
            .route_witness(staged.token.as_ref().unwrap())
            .unwrap()
            .incarnation
    );
    staged.ensure_staged_v2().unwrap();
    assert_eq!(
        registry.route_witness(&previous),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    let successor = staged.token.as_ref().unwrap().clone();
    assert_eq!(
        registry.route_witness(&successor).unwrap().lifecycle,
        SlotLifecycleV1::Staged
    );
    staged.remove_v2().unwrap();
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_explicit_cleanup_trips_emergency_once() {
    let (port, registry) = staging_port("runtime-process:explicit-failure");
    let route = route("runtime-process:explicit-failure");
    let (emergency, trips) = emergency();
    let staged = authority(
        port.install_staged_route_v2(route.clone(), fence(1), emergency)
            .unwrap(),
    );
    let token = staged.token.as_ref().unwrap().clone();
    registry.activate(&token, route.identity()).unwrap();

    assert!(matches!(
        staged.remove_v2(),
        Err(RuntimeRegistryStagingErrorV2::Registry(
            ServingSlotRegistryError::NotDraining
        ))
    ));
    assert_eq!(trips.load(Ordering::SeqCst), 1);
    assert_eq!(
        registry.route_witness(&token).unwrap().lifecycle,
        SlotLifecycleV1::Serving
    );
}

#[test]
fn failed_drop_cleanup_trips_emergency_once() {
    let (port, registry) = staging_port("runtime-process:drop-failure");
    let route = route("runtime-process:drop-failure");
    let (emergency, trips) = emergency();
    let staged = authority(
        port.install_staged_route_v2(route.clone(), fence(1), emergency)
            .unwrap(),
    );
    let token = staged.token.as_ref().unwrap().clone();
    registry.activate(&token, route.identity()).unwrap();

    drop(staged);

    assert_eq!(trips.load(Ordering::SeqCst), 1);
    assert_eq!(
        registry.route_witness(&token).unwrap().lifecycle,
        SlotLifecycleV1::Serving
    );
}

#[test]
fn witness_failure_drops_authority_and_removes_the_installed_route() {
    let (_, registry) = staging_port("runtime-process:witness-failure");
    let route = route("runtime-process:witness-failure");
    let receipt = registry
        .install(route.slot_key(), route.clone(), fence(1))
        .unwrap();
    let token = receipt.token.clone();
    let (emergency, trips) = emergency();
    let staged = RuntimeRegistryStagedRouteV2 {
        registry: registry.clone(),
        identity: route.identity().clone(),
        token: Some(receipt.token),
        emergency,
    };
    let atomic = registry.atomic_observation_v2(token.key());
    let registry_observation = registry.recovery_observation_v2();

    let result = complete_staged_install_v2(
        RuntimeRegistryStagedInstallOutcomeV2::Installed,
        staged,
        Err(ServingSlotRegistryError::RegistryObservationInvalid),
        atomic,
        registry_observation,
    );

    assert!(matches!(
        result,
        Err(RuntimeRegistryStagingErrorV2::Registry(
            ServingSlotRegistryError::RegistryObservationInvalid
        ))
    ));
    assert_eq!(
        registry.route_witness(&token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn barrier_b_activation_returns_exact_evidence_and_linear_serving_authority() {
    let (port, registry) = staging_port("runtime-process:barrier-b");
    let route = route("runtime-process:barrier-b");
    let (emergency, trips) = emergency();
    let replacement = authority(
        port.install_staged_route_v2(route.clone(), fence(1), emergency)
            .unwrap(),
    )
    .into_replacement_v2();
    replacement
        .transition_predecessor_to_draining_v2(None)
        .unwrap();
    replacement.remove_drained_predecessor_v2().unwrap();

    let activation = replacement.activate_barrier_b_v2().unwrap();

    assert_eq!(
        activation.evidence_v2().outcome_v2(),
        RuntimeRegistryBarrierBActivationOutcomeV2::Activated
    );
    assert_eq!(activation.evidence_v2().identity_v2(), route.identity());
    assert_eq!(activation.evidence_v2().fencing_token_v2(), fence(1));
    assert_eq!(activation.evidence_v2().active_interactions_v2(), 0);
    assert_eq!(
        format!("{activation:?}"),
        "RuntimeRegistryBarrierBActivationV2(<redacted>)"
    );
    let (evidence, mut authority) = activation.into_parts_v2();
    assert_eq!(authority.identity_v2(), evidence.identity_v2());
    assert_eq!(authority.fencing_token_v2(), evidence.fencing_token_v2());
    assert_eq!(
        authority.route_incarnation_v2(),
        evidence.route_incarnation_v2()
    );
    assert_eq!(
        authority.activation_sequence_v2(),
        evidence.activation_sequence_v2()
    );
    assert!(evidence.admission_generation_v2().get() > 0);
    assert!(evidence.slot_observation_sequence_v2().get() > 0);
    assert_eq!(
        format!("{authority:?}"),
        "RuntimeRegistryBarrierBServingAuthorityV2(<redacted>)"
    );
    authority.ensure_exact_serving_v2().unwrap();
    registry.begin_drain(&authority.token).unwrap();
    registry.remove(&authority.token).unwrap();
    authority.armed = false;
    drop(authority);
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn barrier_b_rejects_non_final_predecessor_and_drop_cleans_the_staged_route() {
    let (port, registry) = staging_port("runtime-process:barrier-b-not-final");
    let route = route("runtime-process:barrier-b-not-final");
    let (emergency, trips) = emergency();
    let staged = authority(
        port.install_staged_route_v2(route, fence(1), emergency)
            .unwrap(),
    );
    let token = staged.token.as_ref().unwrap().clone();
    let replacement = staged.into_replacement_v2();
    replacement
        .transition_predecessor_to_draining_v2(None)
        .unwrap();

    assert!(matches!(
        replacement.activate_barrier_b_v2(),
        Err(RuntimeRegistryBarrierBActivationErrorV2::PredecessorNotFinal)
    ));
    assert_eq!(
        registry.route_witness(&token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn barrier_b_accepts_only_the_exact_already_serving_replay() {
    let (port, registry) = staging_port("runtime-process:barrier-b-replay");
    let route = route("runtime-process:barrier-b-replay");
    let (emergency, trips) = emergency();
    let replacement = authority(
        port.install_staged_route_v2(route.clone(), fence(1), emergency)
            .unwrap(),
    )
    .into_replacement_v2();
    replacement
        .transition_predecessor_to_draining_v2(None)
        .unwrap();
    replacement.remove_drained_predecessor_v2().unwrap();
    let token = replacement
        .state
        .lock()
        .unwrap()
        .staged
        .token
        .as_ref()
        .unwrap()
        .clone();
    registry.activate(&token, route.identity()).unwrap();

    let (evidence, mut authority) = replacement.activate_barrier_b_v2().unwrap().into_parts_v2();

    assert_eq!(
        evidence.outcome_v2(),
        RuntimeRegistryBarrierBActivationOutcomeV2::AlreadyServing
    );
    assert_eq!(
        registry.route_witness(&token).unwrap().incarnation,
        evidence.route_incarnation_v2()
    );
    authority.ensure_exact_serving_v2().unwrap();
    registry.begin_drain(&authority.token).unwrap();
    registry.remove(&authority.token).unwrap();
    authority.armed = false;
    drop(authority);
    assert_eq!(trips.load(Ordering::SeqCst), 0);
}

#[test]
fn barrier_b_activation_failure_preserves_emergency_cleanup() {
    let (port, registry) = staging_port("runtime-process:barrier-b-failure");
    let route = route("runtime-process:barrier-b-failure");
    let (emergency, trips) = emergency();
    let replacement = authority(
        port.install_staged_route_v2(route.clone(), fence(1), emergency)
            .unwrap(),
    )
    .into_replacement_v2();
    replacement
        .transition_predecessor_to_draining_v2(None)
        .unwrap();
    replacement.remove_drained_predecessor_v2().unwrap();
    let token = replacement
        .state
        .lock()
        .unwrap()
        .staged
        .token
        .as_ref()
        .unwrap()
        .clone();
    registry.activate(&token, route.identity()).unwrap();
    registry.begin_drain(&token).unwrap();

    assert!(matches!(
        replacement.activate_barrier_b_v2(),
        Err(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid)
    ));
    assert_eq!(
        registry.route_witness(&token),
        Err(ServingSlotRegistryError::StaleMutationToken)
    );
    assert_eq!(trips.load(Ordering::SeqCst), 1);
}

#[test]
fn serving_authority_detects_exact_route_loss_and_drop_trips_emergency() {
    let (port, registry) = staging_port("runtime-process:barrier-b-loss");
    let route = route("runtime-process:barrier-b-loss");
    let (emergency, trips) = emergency();
    let replacement = authority(
        port.install_staged_route_v2(route, fence(1), emergency)
            .unwrap(),
    )
    .into_replacement_v2();
    replacement
        .transition_predecessor_to_draining_v2(None)
        .unwrap();
    replacement.remove_drained_predecessor_v2().unwrap();
    let (_, authority) = replacement.activate_barrier_b_v2().unwrap().into_parts_v2();
    registry.begin_drain(&authority.token).unwrap();

    assert_eq!(
        authority.ensure_exact_serving_v2(),
        Err(RuntimeRegistryBarrierBActivationErrorV2::ExactServingLost)
    );
    registry.remove(&authority.token).unwrap();
    assert_eq!(
        authority.ensure_exact_serving_v2(),
        Err(RuntimeRegistryBarrierBActivationErrorV2::ExactServingLost)
    );
    drop(authority);
    assert_eq!(trips.load(Ordering::SeqCst), 1);
}
