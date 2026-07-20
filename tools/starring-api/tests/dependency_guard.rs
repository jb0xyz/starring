const MANIFEST: &str = include_str!("../Cargo.toml");
const LIBRARY: &str = include_str!("../src/lib.rs");
const INPUT: &str = include_str!("../src/input.rs");
const ERROR: &str = include_str!("../src/error.rs");
const PROJECTION: &str = include_str!("../src/projection.rs");
const WORKSPACE: &str = include_str!("../../../Cargo.toml");

fn sources() -> [(&'static str, &'static str); 4] {
    [
        ("lib.rs", LIBRARY),
        ("input.rs", INPUT),
        ("error.rs", ERROR),
        ("projection.rs", PROJECTION),
    ]
}

#[test]
fn edge_mapping_package_has_no_infrastructure_or_raw_store_dependency() {
    for forbidden in ["sqlx", "twilight", "axum", "tower", "reqwest"] {
        let prefix = format!("{forbidden} =");
        assert!(!MANIFEST
            .lines()
            .any(|line| line.trim_start().starts_with(&prefix)));
    }
    for (_, source) in sources() {
        for forbidden in [
            "sqlx::",
            "twilight_",
            "PostgresProductPromotions",
            "PostgresProductDecisions",
            "PromotionService",
            "PromotionStore ",
            "PendingActivationPort ",
            "RuleSetStore ",
            "ActivationRequestStore",
        ] {
            assert!(!source.contains(forbidden));
        }
    }
}

#[test]
fn package_is_library_only_and_registered_once() {
    assert!(!MANIFEST.contains("[[bin]]"));
    assert!(!MANIFEST.contains("src/main.rs"));
    assert_eq!(WORKSPACE.matches("\"tools/starring-api\"").count(), 1);
    assert!(LIBRARY.contains("mod error;"));
    assert!(LIBRARY.contains("mod input;"));
    assert!(LIBRARY.contains("mod projection;"));
}

#[test]
fn mapping_sources_are_comment_free_and_avoid_unsafe_code() {
    for (name, source) in sources() {
        assert!(!source.contains("//"), "{name}");
        assert!(!source.contains("/*"), "{name}");
        assert!(!source.contains("*/"), "{name}");
        assert!(!source.contains("unsafe"), "{name}");
    }
}

#[test]
fn edge_contract_keeps_typed_inputs_closed_errors_and_v2_projection() {
    let contract = [LIBRARY, INPUT, ERROR, PROJECTION].concat();
    for required in [
        "MappedPromoteCommand",
        "MappedApproveCommand",
        "MappedRejectCommand",
        "MappedApplyCommand",
        "map_authoring_application_error",
        "map_product_application_error",
        "project_promotion",
        "project_deployment_operational_v2",
        "system_time_to_utc",
    ] {
        assert!(contract.contains(required));
    }
    assert!(!PROJECTION.contains("DateTime::<Utc>::from("));
    assert!(PROJECTION.contains("revision: decision.revision().get()"));
    assert!(!PROJECTION.contains("revision: promotion.revision()"));
    assert!(PROJECTION.contains("DeploymentConvergencePhaseV2::Cancelled"));
    assert!(PROJECTION.contains("DeploymentServingFreshnessV2::IdentityMismatch"));
}
