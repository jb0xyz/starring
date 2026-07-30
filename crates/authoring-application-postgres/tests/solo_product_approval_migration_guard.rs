const MIGRATION: &str =
    include_str!("../../../migrations/202607310001_enforce_solo_product_approval.sql");
const MANIFEST_MIGRATION: &str =
    include_str!("../../../migrations/202607310002_refresh_solo_approval_schema_manifests.sql");
const READINESS_MIGRATION: &str =
    include_str!("../../../migrations/202607310003_refresh_solo_approval_readiness_pins.sql");
const APPROVAL_SOURCE: &str =
    include_str!("../../../migrations/202607190009_separate_product_binding_identities.sql");
const REPAIR_SOURCE: &str =
    include_str!("../../../migrations/202607200002_scope_product_promotion_execution.sql");

#[test]
fn solo_product_approval_migration_is_fail_closed_and_capability_neutral() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    for required in [
        "LOCK TABLE public.automation_installation_authority_versions",
        "LOCK TABLE public.activation_requests",
        "LOCK TABLE public.activation_request_approvals",
        "authority.required_approvals <> 1",
        "activation.authority_kind = 'product_authoring'",
        "activation.required_approvals <> 1",
        "HAVING pg_catalog.count(*) > 1",
        "installation_authority_single_approval",
        "CHECK (required_approvals = 1) NOT VALID",
        "activation_requests_product_single_approval",
        "authority_kind <> 'product_authoring'",
        "OR required_approvals = 1",
        "VALIDATE CONSTRAINT installation_authority_single_approval",
        "VALIDATE CONSTRAINT activation_requests_product_single_approval",
        "pg_catalog.pg_get_functiondef",
        "pg_catalog.to_regprocedure",
        "metadata_after IS DISTINCT FROM metadata_before",
        "'security_definer', function_row.prosecdef",
        "'parallel', function_row.proparallel",
        "'config', pg_catalog.to_jsonb(function_row.proconfig)",
    ] {
        assert!(
            MIGRATION.contains(required),
            "missing solo product approval migration guard: {required}"
        );
    }
    for forbidden in [
        "GRANT ",
        "DROP FUNCTION",
        "DELETE FROM",
        "UPDATE public.",
        "required_approvals = 2",
    ] {
        assert!(
            !MIGRATION.contains(forbidden),
            "forbidden solo product approval migration edge: {forbidden}"
        );
    }
}

#[test]
fn solo_product_approval_migration_targets_both_historical_gates_exactly() {
    let approval_gate = "IF activation_row.requester_id = expected_acting_user_id THEN";
    let repair_gate = "OR approval.approver_id = activation_row.requester_id";
    assert_eq!(APPROVAL_SOURCE.matches(approval_gate).count(), 1);
    assert_eq!(REPAIR_SOURCE.matches(repair_gate).count(), 1);
    assert_eq!(MIGRATION.matches(approval_gate).count(), 1);
    assert_eq!(MIGRATION.matches(repair_gate).count(), 1);
    assert_eq!(MIGRATION.matches("self_approval_forbidden").count(), 2);
    assert!(MIGRATION.contains("EXECUTE pg_catalog.replace(function_definition"));
}

#[test]
fn solo_product_approval_refreshes_every_affected_runtime_manifest() {
    assert!(!MANIFEST_MIGRATION.contains("--"));
    assert!(!MANIFEST_MIGRATION.contains("/*"));
    assert!(!MANIFEST_MIGRATION.contains("//"));
    for required in [
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "RETURN observed_count = 358",
        "RETURN observed_count = 969",
        "RETURN observed_count = 492",
        "aee90f2f78d8106e298c8075b0710bca6d47b3b37cc9d2c6598a4f9f769b9f7d",
        "metadata_after IS DISTINCT FROM metadata_before",
        "pg_catalog.pg_get_functiondef",
        "solo approval schema manifest postflight failed",
    ] {
        assert!(
            MANIFEST_MIGRATION.contains(required),
            "missing solo approval schema manifest guard: {required}"
        );
    }
    for forbidden in ["GRANT ", "DROP FUNCTION", "DELETE FROM", "UPDATE public."] {
        assert!(
            !MANIFEST_MIGRATION.contains(forbidden),
            "forbidden solo approval schema manifest edge: {forbidden}"
        );
    }
}

#[test]
fn solo_product_approval_cascades_manifest_pins_into_runtime_readiness() {
    assert!(!READINESS_MIGRATION.contains("--"));
    assert!(!READINESS_MIGRATION.contains("/*"));
    assert!(!READINESS_MIGRATION.contains("//"));
    for required in [
        "public.starring_runtime_execution_database_readiness_v1()",
        "public.starring_runtime_exact_target_database_readiness_v2()",
        "public.starring_runtime_serving_database_readiness_v1()",
        "1d00ad69e8c2633713f35670b831d274329d24fb3e7410b13d429a19b5fb7c34",
        "2b28b5bac9a444333d1681ccc158243d8a0d010818fa0719374699f8d0275c43",
        "977410de87917e582c6018c0ddcea164045b82a4550fb166ff138e3efa65238d",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_digest <> readiness_digests[function_index]",
    ] {
        assert!(
            READINESS_MIGRATION.contains(required),
            "missing solo approval readiness guard: {required}"
        );
    }
    for forbidden in ["GRANT ", "DROP FUNCTION", "DELETE FROM", "UPDATE public."] {
        assert!(
            !READINESS_MIGRATION.contains(forbidden),
            "forbidden solo approval readiness edge: {forbidden}"
        );
    }
}
