const MIGRATION: &str = include_str!(
    "../../../migrations/202607270002_fence_legacy_runtime_certification_reservation_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");
const SUCCESSOR_READINESS_DIGEST: &str =
    "a57602a79ee2aa5ac884dffb56d152bb5721d111e07eac5a5f853952d6db214f";
const LATEST_READINESS_DIGEST: &str =
    "a5191ef59e5365476860af1150a176049ef00c5b0d6c3f7cfe40e0b5be9d738a";

const LEGACY_ENTRYPOINTS: [&str; 6] = [
    "public.starring_runtime_execution_claim_next_v1(text,bigint)",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_recover_stale_live_v1()",
];

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

#[test]
fn migration_is_atomic_quiesced_acl_preserving_and_surface_closed() {
    let barrier = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let snapshot = MIGRATION
        .find("starring_runtime_legacy_certification_fence_snapshot")
        .unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let claim = MIGRATION.find("DO $patch_claim$").unwrap();
    let direct = MIGRATION.find("DO $patch_direct_writers$").unwrap();
    let recover = MIGRATION.find("DO $patch_recover$").unwrap();
    let manifest = MIGRATION.find("DO $patch_manifest$").unwrap();
    let readiness = MIGRATION.find("DO $patch_readiness$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(barrier < table_lock);
    assert!(table_lock < snapshot);
    assert!(snapshot < preflight);
    assert!(preflight < claim);
    assert!(claim < direct);
    assert!(direct < recover);
    assert!(recover < manifest);
    assert!(manifest < readiness);
    assert!(readiness < postflight);
    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '30s';",
        "IN ACCESS EXCLUSIVE MODE;",
        "public.runtime_certification_operations_v2",
        "function_oid OID PRIMARY KEY",
        "function_owner OID NOT NULL",
        "function_acl ACLITEM[]",
        "function_row.proowner IS DISTINCT FROM snapshot.function_owner",
        "function_row.proacl IS DISTINCT FROM snapshot.function_acl",
        "runtime_legacy_certification_fence_executor_not_quiesced",
        "NOT role.rolcanlogin",
        "FROM pg_catalog.pg_auth_members AS membership",
        "FROM pg_catalog.pg_stat_activity AS activity",
        "activity.backend_type = 'client backend'",
        "FROM pg_catalog.pg_prepared_xacts AS prepared",
        "prepared.database = pg_catalog.current_database()",
        "RESET statement_timeout;",
        "RESET lock_timeout;",
        "RESET search_path;",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for forbidden in [
        "--",
        "/*",
        "//",
        "GRANT ",
        "ALTER DEFAULT PRIVILEGES",
        "CREATE FUNCTION ",
        "CREATE OR REPLACE FUNCTION ",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn exactly_the_six_existing_legacy_entrypoints_are_snapshotted_and_patched() {
    let snapshot = MIGRATION
        .split("INSERT INTO pg_temp.starring_runtime_legacy_certification_fence_snapshot")
        .nth(1)
        .unwrap()
        .split("DO $preflight$")
        .next()
        .unwrap();
    for entrypoint in LEGACY_ENTRYPOINTS {
        assert_eq!(snapshot.matches(entrypoint).count(), 1, "{entrypoint}");
    }
    assert!(snapshot.contains("public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(snapshot.contains("public.starring_runtime_execution_database_readiness_v1()"));
    assert!(MIGRATION.contains("FROM pg_temp.starring_runtime_legacy_certification_fence_snapshot"));
    assert!(MIGRATION.contains(") <> 8"));

    let claim = dollar_block("patch_claim");
    let direct = dollar_block("patch_direct_writers");
    let recover = dollar_block("patch_recover");
    assert!(claim.contains(LEGACY_ENTRYPOINTS[0]));
    for entrypoint in &LEGACY_ENTRYPOINTS[1..5] {
        assert_eq!(direct.matches(entrypoint).count(), 1, "{entrypoint}");
    }
    assert!(recover.contains(LEGACY_ENTRYPOINTS[5]));
}

#[test]
fn reservation_ownership_is_permanent_deployment_scope_only() {
    let claim = dollar_block("patch_claim");
    let direct = dollar_block("patch_direct_writers");
    let recover = dollar_block("patch_recover");

    for field in ["tenant_id", "installation_id", "deployment_id"] {
        assert_eq!(
            claim
                .matches(&format!("reservation.{field} = deployment.{field}"))
                .count(),
            2,
            "claim selector {field}"
        );
        assert_eq!(
            claim
                .matches(&format!("reservation.{field} = deployment_row.{field}"))
                .count(),
            2,
            "claim locked {field}"
        );
        assert_eq!(
            direct
                .matches(&format!("reservation.{field} = deployment_row.{field}"))
                .count(),
            1,
            "direct {field}"
        );
        assert_eq!(
            recover
                .matches(&format!("reservation.{field} = deployment.{field}"))
                .count(),
            1,
            "recover selector {field}"
        );
        assert_eq!(
            recover
                .matches(&format!("reservation.{field} = deployment_row.{field}"))
                .count(),
            1,
            "recover locked {field}"
        );
    }

    for block in [claim, direct, recover] {
        for forbidden in [
            "reservation.deployment_revision",
            "reservation.convergence_attempt_no",
            "reservation.operation_id",
            "reservation.intent_fingerprint",
        ] {
            assert!(!block.contains(forbidden), "{forbidden}");
        }
    }
}

#[test]
fn selectors_prefilter_and_definitively_recheck_after_the_deployment_lock() {
    let claim = dollar_block("patch_claim");
    let recover = dollar_block("patch_recover");
    assert_eq!(claim.matches("AND NOT EXISTS (").count(), 2);
    assert_eq!(claim.matches("IF EXISTS (").count(), 2);
    assert_eq!(recover.matches("AND NOT EXISTS (").count(), 1);
    assert_eq!(recover.matches("IF EXISTS (").count(), 1);
    assert!(!claim.contains("RX001"));
    assert!(!recover.contains("RX001"));
    for block in [claim, recover] {
        assert!(block.contains("RAISE no_data_found;"));
        assert!(block.contains("FROM public.runtime_certification_operations_v2 AS reservation"));
    }

    let postflight = dollar_block("postflight");
    for required in [
        "reserved_guard_count <> 4",
        "reserved_guard_count <> 2",
        "'FOR UPDATE;'",
        "'FOR UPDATE SKIP LOCKED;'",
        "'WHERE reservation.tenant_id = deployment.tenant_id'",
        "'WHERE reservation.tenant_id = deployment_row.tenant_id'",
        "runtime_legacy_certification_fence_claim_contract_drift",
        "runtime_legacy_certification_fence_recover_contract_drift",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}

#[test]
fn direct_writers_reject_reserved_deployments_with_the_existing_rx001_class() {
    let direct = dollar_block("patch_direct_writers");
    assert_eq!(direct.matches("ERRCODE = ''RX001''").count(), 1);
    assert_eq!(
        direct
            .matches("FROM public.runtime_certification_operations_v2 AS reservation")
            .count(),
        1
    );
    for required in [
        "'renew'",
        "'mutate'",
        "'certify_prepare'",
        "'certify_commit'",
        "IF EXISTS (",
        "MESSAGE = ''' || patch_row.ownership_message",
        "runtime_legacy_certification_fence_",
        "_patch_drift",
    ] {
        assert!(direct.contains(required), "{required}");
    }

    let postflight = dollar_block("postflight");
    assert!(postflight.contains("reserved_guard_count <> 1"));
    assert!(postflight.contains("runtime_legacy_certification_fence_direct_contract_drift"));
}

#[test]
fn every_definitive_reservation_check_follows_the_canonical_lock_order() {
    let postflight = dollar_block("postflight");
    for required in [
        "writer_position < controller_position",
        "controller_position < slot_position",
        "writer_position < slot_position",
        "slot_position < physical_position",
        "physical_position < deployment_lock_position",
        "deployment_lock_position < reservation_position",
        "reservation_position < continuation_position",
        "'starring_runtime_writer_fence_observe_v1'",
        "'starring-runtime-execution-controller-v1:'",
        "'starring-runtime-serving-slot-v1:'",
        "'starring_runtime_slot_writer_fence_lock_v2'",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
    assert_eq!(
        postflight
            .matches("physical_position < deployment_lock_position")
            .count(),
        3
    );
    assert_eq!(
        postflight
            .matches("deployment_lock_position < reservation_position")
            .count(),
        3
    );
}

#[test]
fn predecessor_and_successor_definition_pins_cascade_through_readiness() {
    let preflight = dollar_block("preflight");
    for digest in [
        "7cb6550864ed68e136e6e6b48c8cce59d179d895e3919a6abca77b7dfc7a4990",
        "3c418773f843f2bf8827464624b4fd3124d8979c4c15b4323a96e76676c11c4e",
        "76d965851a753501722854c0aecc22d51a3eaa92e93d55a299bfd59d5d922559",
        "0b9fdc77ec2d85ea2513d6edf462ddddfe3304c4c1f53bec0432b5e0180e6967",
        "5f0fa2982466bf5ca30250d2334c8314fa0c510ae586f6b750cdd1b662655fe1",
        "635aab9493e1fd2ad8a138633d6447f88752589414a27c2bad4e56afdd22f932",
        "4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7",
        "6962c1c2ffdd862a86aed3c84569ac50307964d59711d0bddc26aadbf68577e2",
    ] {
        assert!(preflight.contains(digest), "{digest}");
    }

    let manifest = dollar_block("patch_manifest");
    assert!(manifest.contains("RETURN observed_count = 650"));
    assert!(manifest.contains("f053e9131dcd32f1168ff6201ad57f4f40e3165ab619414a3552b74717bbe2c9"));
    assert!(manifest.contains("65c41a8e67ec225e567403f2f24eba8e31964a51d1a1ce484774cae3db5bd58c"));
    let readiness = dollar_block("patch_readiness");
    assert!(readiness.contains("4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7"));
    assert!(readiness.contains("ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4"));

    let postflight = dollar_block("postflight");
    assert!(postflight.contains("definition_digest"));
    assert!(postflight.contains("pg_catalog.pg_get_functiondef(function_row.oid)"));
    assert!(postflight.contains("snapshot_mismatch_count"));
    for digest in [
        "cc5475b256b6b48f3c4f6d3933461cdcdeff1dbdb974d32d7d735348d8f14eb4",
        "00fb1426fd8711b496b35e0658db13a534560ba13191d710c4274cd54461275c",
        "9e201e149dac432794bfcfc23b424f59741869fcf9d39765693a21b2451646ce",
        "9be2d9b8c329665cea635e8a44144aabe58ed684d3d227eb60ad583f78640269",
        "5c1b3c8c50e3a2d3d0f0149bf408fca51069db975573b3375f0f76bc1e5c159c",
        "b30467f0d866bbcadb82bd6322e5d169aec4c443770c896660b885aa3e3b7457",
        "ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4",
        SUCCESSOR_READINESS_DIGEST,
    ] {
        assert!(postflight.contains(digest), "{digest}");
    }
    assert!(!MIGRATION.contains("0000000000000000000000000000000000000000000000000000000000000000"));
    assert!(CONTRACT_SOURCE.contains(LATEST_READINESS_DIGEST));
    assert!(DATABASE_SOURCE.contains(LATEST_READINESS_DIGEST));
    assert!(SECURITY_SUPPORT_SOURCE.contains(LATEST_READINESS_DIGEST));
}
