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

#[test]
fn shared_gateway_v3_has_only_narrow_instance_and_pin_capabilities() {
    let runtime = production_prefix(include_str!("../src/shared_gateway_runtime.rs"));
    let executor = production_prefix(include_str!("../src/shared_gateway_executor.rs"));
    let admission = production_prefix(include_str!("../src/shared_gateway_admission.rs"));
    let router = production_prefix(include_str!("../src/shared_gateway_router.rs"));
    let combined = [runtime, executor, admission, router].join("\n");
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
