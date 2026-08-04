fn production_prefix(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap()
}

fn assert_forbidden_calls_absent(source: &str) {
    for forbidden in [
        ".list_by_guild(",
        ".list_deleting(",
        ".update_status(",
        ".transition_to_deleting(",
        ".mark_deleted(",
        ".publish(",
        ".activate(",
        ".activate_guarded(",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
}

fn assert_test_only(source: &str, declaration: &str) {
    let declaration = source.find(declaration).unwrap();
    let attributes = source[..declaration].rsplit("\n\n").next().unwrap();
    assert!(
        attributes.contains("#[cfg(any(test, doctest))]"),
        "{declaration}"
    );
}

#[test]
fn test_only_shared_gateway_v3_has_only_narrow_instance_and_pin_capabilities() {
    let runtime = production_prefix(include_str!("../src/shared_gateway_runtime.rs"));
    let executor = production_prefix(include_str!("../src/shared_gateway_executor.rs"));
    let admission = production_prefix(include_str!("../src/shared_gateway_admission.rs"));
    let dispatcher = production_prefix(include_str!("../src/shared_gateway_dispatcher.rs"));
    let router = production_prefix(include_str!("../src/shared_gateway_router.rs"));
    let combined = [runtime, executor, admission, dispatcher, router].join("\n");
    assert!(runtime.contains("PinnedInstanceResolverV1"));
    assert!(runtime.contains("InstanceRegistrarV1"));
    assert!(runtime.contains("InstanceRouteReaderV1"));
    assert!(executor.contains("handle_interaction_with_resolver_v1"));
    assert!(admission.contains("InstanceRouteReaderV1"));
    assert!(router.contains("read_instance_route_v1"));
    assert!(!combined.contains("RuleSetStore"));
    assert!(!combined.contains("&impl InstanceStore"));
    assert!(!combined.contains("I: InstanceStore"));
    assert_forbidden_calls_absent(&combined);
}

#[test]
fn test_only_shard_runtime_delegates_to_the_source_neutral_dispatcher() {
    let runtime = production_prefix(include_str!("../src/shared_gateway_runtime.rs"));
    let dispatcher = production_prefix(include_str!("../src/shared_gateway_dispatcher.rs"));
    assert!(runtime.contains("SharedGatewayInteractionEnvelopeV3::from_twilight_interaction_v3"));
    assert!(runtime.contains("reserve_shared_gateway_interaction_v3("));
    assert!(runtime.contains("dispatch_reserved_shared_gateway_interaction_v3("));
    for forbidden in [
        "parse_shared_gateway_route_v1",
        "admission_budget.try_reserve",
        "execute_admitted_interaction_v3(",
    ] {
        assert!(!runtime.contains(forbidden), "{forbidden}");
    }
    assert!(!dispatcher.contains("twilight_gateway"));
    assert!(!dispatcher.contains("Shard"));
    assert!(dispatcher.contains("token: SharedGatewayInteractionTokenV3"));
    assert!(dispatcher.contains("pub struct SharedGatewayInteractionTokenV3(Zeroizing<String>)"));
    assert!(dispatcher.contains("pub(crate) fn from_twilight_interaction_v3("));
    assert!(dispatcher.contains("impl Drop for ZeroizingTwilightInteractionV3"));
    assert!(dispatcher.contains("zeroize_twilight_interaction_v3(&mut self.0)"));
    assert!(dispatcher.contains(
        "pub fn reserve_shared_gateway_interaction_v3(\n    envelope: SharedGatewayInteractionEnvelopeV3,"
    ));
}

#[test]
fn legacy_http_execution_surfaces_are_absent_from_production_compilation() {
    let library = include_str!("../src/lib.rs");
    let dispatcher = include_str!("../src/shared_gateway_dispatcher.rs");
    let executor = include_str!("../src/shared_gateway_executor.rs");
    assert!(library.contains("mod shared_gateway_executor;"));
    assert!(!library.contains("pub mod shared_gateway_executor;"));
    assert!(library.contains("#[cfg(any(test, doctest))]\npub mod shared_gateway_runtime;"));
    assert!(library.contains(
        "#[cfg(any(test, doctest))]\npub use shared_gateway_dispatcher::{\n    acknowledge_shared_gateway_interaction_rejection_v3,"
    ));
    assert!(library.contains(
        "#[cfg(any(test, doctest))]\npub use shared_gateway_executor::execute_admitted_interaction_v3;"
    ));
    assert!(library.contains("#[cfg(any(test, doctest))]\npub use shared_gateway_runtime::{"));
    for declaration in [
        "pub enum SharedGatewayInteractionDispatchOutcomeV3",
        "pub enum SharedGatewayRejectionAcknowledgementOutcomeV3",
        "pub async fn acknowledge_shared_gateway_interaction_rejection_v3(",
        "pub async fn dispatch_reserved_shared_gateway_interaction_v3<",
        "pub async fn dispatch_v3(",
        "pub async fn acknowledge_rejection_v3(",
    ] {
        assert_test_only(dispatcher, declaration);
    }
    assert_test_only(executor, "pub async fn execute_admitted_interaction_v3(");
    assert!(dispatcher.contains("pub fn reserve_shared_gateway_interaction_v3("));
    assert!(dispatcher.contains("pub async fn admit_reserved_shared_gateway_interaction_v1<I>("));
    assert!(dispatcher.contains("pub async fn execute_acquired_v1<P>("));
    assert!(executor.contains("pub(crate) fn execution_inputs("));
}

#[test]
fn durable_admission_is_separate_from_http_execution() {
    let dispatcher = production_prefix(include_str!("../src/shared_gateway_dispatcher.rs"));
    let admission = dispatcher
        .split("pub async fn admit_reserved_shared_gateway_interaction_v1")
        .nth(1)
        .unwrap()
        .split("pub fn cancel_reserved_shared_gateway_interaction_v3")
        .next()
        .unwrap();
    assert!(admission.contains("reservation\n        .admit("));
    for forbidden in [
        "Client",
        "interaction_http",
        "mutation_http",
        "TwilightInteractionResponder",
        "TwilightMutationAdapter",
        "execute_acquired_interaction_v1",
        "execute_admitted_interaction_v3",
    ] {
        assert!(!admission.contains(forbidden), "{forbidden}");
    }

    let execution = dispatcher
        .split("pub async fn execute_acquired_v1<P>")
        .nth(1)
        .unwrap()
        .split("pub async fn acknowledge_rejection_v3")
        .next()
        .unwrap();
    assert!(execution.contains("permit: P"));
    assert!(execution.contains("P: AcquiredInteractionLifecyclePermitV1"));
    assert!(execution.contains("execute_acquired_interaction_v1("));
}

#[test]
fn narrow_runner_uses_the_pinned_resolver_boundary() {
    let runner = include_str!("../src/runner.rs");
    let narrow = runner
        .split("pub(crate) async fn handle_interaction_with_resolver_v1")
        .nth(1)
        .unwrap()
        .split("fn static_outcome")
        .next()
        .unwrap();
    assert!(narrow.contains("InstanceRegistrarV1"));
    assert!(narrow.contains("PinnedInstanceResolverV1"));
    assert!(narrow.contains("dispatch_instance_action_with_resolver_v1"));
    assert!(!narrow.contains("RuleSetStore"));
    assert!(!narrow.contains("InstanceStore"));
    assert_forbidden_calls_absent(narrow);
}

#[test]
fn synchronous_invalidation_precedes_every_observable_close_boundary() {
    let control = production_prefix(include_str!("../src/shared_gateway_control.rs"));
    let replace = control
        .split("fn replace_snapshot")
        .nth(1)
        .unwrap()
        .split("fn invalidation_signal")
        .next()
        .unwrap();
    let invalidation = replace
        .find("self.invalidation.invalidate(signal)")
        .unwrap();
    let state = replace.find("self.state = snapshot.connection").unwrap();
    let admission = replace
        .find("self.admission.send_replace(snapshot)")
        .unwrap();
    let connection = replace
        .find("self.connection.send_replace(snapshot.connection)")
        .unwrap();
    assert!(invalidation < state);
    assert!(state < admission);
    assert!(admission < connection);

    let owner_drop = control
        .split("impl Drop for SharedGatewayControlV3")
        .nth(1)
        .unwrap()
        .split("fn issue_ready_lease")
        .next()
        .unwrap();
    assert!(
        owner_drop
            .find("GatewayInvalidationSignalV3::ControlOrphaned")
            .unwrap()
            < owner_drop.find("control_alive.store").unwrap()
    );

    let runtime = production_prefix(include_str!("../src/shared_gateway_runtime.rs"));
    let finish = runtime.split("async fn finish_gateway").nth(1).unwrap();
    assert!(
        finish
            .find("control.begin_runtime_failure_drain()")
            .unwrap()
            < finish.find("shard.close").unwrap()
    );
    for forbidden in ["send_replace", "control_alive.store"] {
        assert!(!runtime.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn barrier_commands_are_reserved_before_pause_and_resume_without_queue_wait() {
    let control = production_prefix(include_str!("../src/shared_gateway_control.rs"));
    let reservation = control
        .split("pub fn try_reserve_barrier_commands_v3(")
        .nth(1)
        .unwrap()
        .split("pub async fn pause_admission_reserved_v3(")
        .next()
        .unwrap();
    let permits = reservation
        .match_indices(".try_reserve_owned()")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(permits.len(), 4);
    let lifecycle_upgrade = reservation.find(".lifecycle_reservations").unwrap();
    let pause_channel = reservation
        .find("let (pause_acknowledgement, pause_observation) = oneshot::channel()")
        .unwrap();
    let resume_channel = reservation
        .find("let (resume_acknowledgement, resume_observation) = oneshot::channel()")
        .unwrap();
    assert!(permits[0] < permits[1]);
    assert!(permits[1] < lifecycle_upgrade);
    assert!(lifecycle_upgrade < permits[2]);
    assert!(permits[2] < permits[3]);
    assert!(permits[3] < pause_channel);
    assert!(pause_channel < resume_channel);

    let pause = control
        .split("pub async fn pause_admission_reserved_v3(")
        .nth(1)
        .unwrap()
        .split("pub async fn resume_admission_reserved_v3(")
        .next()
        .unwrap();
    assert!(pause.contains("pause.permit.send(GatewayCommandV3::PauseReserved"));
    assert!(pause.contains("lifecycle: pause.lifecycle"));
    assert!(!pause.contains("oneshot::channel"));
    assert!(!pause.contains("self.commands.send"));
    assert!(!pause.contains("try_send"));
    assert!(!pause.contains("reserve_owned"));

    let resume = control
        .split("pub async fn resume_admission_reserved_v3(")
        .nth(1)
        .unwrap()
        .split("pub async fn pause_admission(")
        .next()
        .unwrap();
    assert!(resume.contains("GatewayCommandV3::ResumeReserved"));
    assert!(resume.contains("lifecycle: reservation.resume.lifecycle"));
    assert!(!resume.contains("oneshot::channel"));
    assert!(!resume.contains("self.commands.send"));
    assert!(!resume.contains("try_send"));
    assert!(!resume.contains("reserve_owned"));

    let runtime_pause = control
        .split("fn pause_reserved(")
        .nth(1)
        .unwrap()
        .split("fn resume(")
        .next()
        .unwrap();
    let runtime_resume = control
        .split("fn resume_reserved(")
        .nth(1)
        .unwrap()
        .split("fn drain(")
        .next()
        .unwrap();
    for transition in [runtime_pause, runtime_resume] {
        assert!(transition.contains("self.publish_reserved_transition("));
        assert!(!transition.contains("self.publish_transition("));
        assert!(!transition.contains("try_reserve"));
        assert!(!transition.contains("reserve_owned"));
        assert!(!transition.contains("oneshot::channel"));
        assert!(!transition.contains(".await"));
    }
    let publication = control
        .split("fn publish_reserved_transition(")
        .nth(1)
        .unwrap()
        .split("fn fail_closed(")
        .next()
        .unwrap();
    assert!(publication.contains("lifecycle.send(event)"));
    assert!(!publication.contains("try_reserve"));
    assert!(!publication.contains("reserve_owned"));
    assert!(!publication.contains("oneshot::channel"));
    assert!(!publication.contains(".await"));
    assert!(control.contains("lifecycle_reservations: mpsc::WeakSender<GatewayLifecycleEventV3>"));

    for authority in [
        "GatewayBarrierCommandReservationV3",
        "GatewayReservedResumeCommandV3",
    ] {
        let prefix = control
            .split(&format!("pub struct {authority} {{"))
            .next()
            .unwrap();
        assert!(!prefix.ends_with("#[derive(Clone)]\n"));
        assert!(control.contains(&format!("formatter.write_str(\"{authority}(<redacted>)\")")));
    }
}
