#[test]
fn regular_dependencies_keep_the_stateful_runtime_protocol_pure() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    for forbidden in ["ai-gateway", "ai_gateway", "llm", "sqlx", "twilight"] {
        assert!(
            !regular.contains(forbidden),
            "forbidden regular dependency: {forbidden}"
        );
    }
}

#[test]
fn production_surface_has_no_free_publication_or_evaluation_proof_constructor() {
    let event = include_str!("../src/event.rs");
    let evaluator = include_str!("../src/evaluator.rs");
    let digest = include_str!("../src/digest.rs");
    assert!(event.contains("#[cfg(test)]\n    pub(crate) fn from_test_authority"));
    assert!(!event.contains("pub fn from_test_authority"));
    assert!(!evaluator.contains("pub fn prepare("));
    assert!(!evaluator.contains("Deserialize"));
    assert!(!evaluator.contains("StatefulSimulationTraceDigestV1"));
    assert!(digest.contains("evaluation: PreparedStatefulEvaluationV1"));
    assert!(!digest.contains("pub fn prepare_test_scaffold"));
}
