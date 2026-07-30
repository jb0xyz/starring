use std::fs;
use std::path::Path;

#[test]
fn serving_heartbeat_monitor_preserves_exact_authority_and_database_contracts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source =
        fs::read_to_string(root.join("src/serving_heartbeat_monitor.rs")).expect("monitor source");

    for required in [
        "RuntimeServingReceiptV2",
        "PostgresRuntimeServingLeaseV1",
        "RuntimeRegistryBarrierBServingMonitorAuthorityV2",
        "RuntimeShutdownTriggerV1",
        "RuntimeShutdownObserverV1",
        "RUNTIME_SERVING_HEARTBEAT_INTERVAL_V2: Duration = Duration::from_secs(15)",
        "RUNTIME_SERVING_HEARTBEAT_LEASE_V2: Duration = Duration::from_secs(45)",
        "registry.observe_exact_serving_v2()",
        "database.heartbeat_serving_v2(&current.identity, config.lease_for)",
        "database.observe_serving_v2(&current.identity)",
        "one_step_successor_identity_v2(&current.identity)",
        "RuntimeServingPersistenceErrorV1::RetryNotReady",
        "RuntimeShutdownCauseV1::IngressAcknowledgementTerminal",
        "actor.abort()",
    ] {
        assert!(source.contains(required), "{required}");
    }

    for forbidden in [
        "sqlx::",
        "HEARTBEAT_V2_QUERY",
        "impl Clone for RuntimeServingHeartbeatMonitor",
        "#[derive(Clone)]\npub(crate) struct RuntimeServingHeartbeatMonitor",
        "RuntimeServingIdentityV2(<",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }

    let handshake = braced_declaration(
        &source,
        "async fn start_runtime_serving_heartbeat_monitor_with_ports_v2",
    );
    let first_registry = handshake
        .find("registry.observe_exact_serving_v2()")
        .unwrap();
    let database = handshake
        .find("database.observe_serving_v2(&receipt.identity)")
        .unwrap();
    let second_registry = handshake[first_registry + 1..]
        .find("registry.observe_exact_serving_v2()")
        .map(|index| first_registry + 1 + index)
        .unwrap();
    assert!(first_registry < database && database < second_registry);

    let recovery = braced_declaration(
        &source,
        "async fn resolve_unknown_runtime_serving_heartbeat_v2",
    );
    let old = recovery
        .find("database.observe_serving_v2(&current.identity)")
        .unwrap();
    let successor_identity = recovery
        .find("one_step_successor_identity_v2(&current.identity)")
        .unwrap();
    let successor = recovery
        .find("database.observe_serving_v2(&successor_identity)")
        .unwrap();
    assert!(old < successor_identity && successor_identity < successor);
}

fn braced_declaration<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source.find(marker).unwrap();
    let body = source[start..]
        .find('{')
        .map(|index| start + index)
        .unwrap();
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for index in body..bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=index];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated declaration")
}
