const CERTIFICATION_MIGRATION: &str =
    include_str!("../../../migrations/202607300003_finalize_runtime_certification_v2.sql");
const HEARTBEAT_MIGRATION: &str =
    include_str!("../../../migrations/202607310013_harden_serving_v2_lifecycle.sql");
const MANIFEST_MIGRATION: &str =
    include_str!("../../../migrations/202607310014_refresh_serving_v2_schema_manifest.sql");
const READINESS_MIGRATION: &str =
    include_str!("../../../migrations/202607310015_refresh_serving_v2_readiness_pin.sql");
const ROLLOVER_MIGRATION: &str =
    include_str!("../../../migrations/202607310016_allow_serving_heartbeat_owner_rollover.sql");
const ROLLOVER_MANIFEST_MIGRATION: &str =
    include_str!("../../../migrations/202607310017_refresh_serving_owner_rollover_manifest.sql");
const ROLLOVER_READINESS_MIGRATION: &str =
    include_str!("../../../migrations/202607310018_refresh_serving_owner_rollover_readiness.sql");
const POST_ATTESTATION_ACK_MIGRATION: &str =
    include_str!("../../../migrations/202607310019_require_post_attestation_serving_ack.sql");
const POST_ATTESTATION_ACK_MANIFEST_MIGRATION: &str = include_str!(
    "../../../migrations/202607310020_refresh_serving_post_attestation_ack_manifest.sql"
);
const POST_ATTESTATION_ACK_READINESS_MIGRATION: &str = include_str!(
    "../../../migrations/202607310021_refresh_serving_post_attestation_ack_readiness.sql"
);
const SERVING_DATABASE: &str =
    include_str!("../../automation-runtime-serving-postgres/src/database.rs");

fn certification_function(name: &str) -> &'static str {
    CERTIFICATION_MIGRATION
        .split(&format!("CREATE FUNCTION public.{name}("))
        .nth(1)
        .unwrap()
        .split("\n$function$;")
        .next()
        .unwrap()
}

#[test]
fn heartbeat_requires_a_strict_successor_with_current_ingress_authority() {
    let heartbeat = certification_function("starring_runtime_serving_heartbeat_v2");
    for required in [
        "owner_row.process_instance_id",
        "IS DISTINCT FROM expected_process_instance_id",
        "owner_row.lease_epoch::TEXT",
        "#>> '{gateway_owner_lease_id,lease_epoch}'",
        "owner_row.expected_build_revision",
        "#>> '{gateway_owner_lease_id,expected_build_revision}'",
        "owner_row.expires_at <= pg_catalog.clock_timestamp()",
        "acknowledgement_row.process_instance_id",
        "acknowledgement_row.owner_lease_epoch",
        "acknowledgement_row.expected_build_revision",
        "acknowledgement_row.observed_owner_revision",
        "IS DISTINCT FROM owner_row.owner_revision",
        "acknowledgement_row.expires_at <= pg_catalog.clock_timestamp()",
    ] {
        assert!(heartbeat.contains(required), "{required}");
    }
    for required in [
        "owner_row.owner_revision::TEXT",
        "IS DISTINCT FROM attestation_row.v2_route_admission",
        "->> ''attested_owner_revision''",
        "owner_row.owner_revision",
        "<= (",
        "attestation_row.v2_route_admission",
        ")::BIGINT",
        "pg_catalog.pg_advisory_xact_lock_shared(",
        "''starring-runtime-writer-fence-v1''",
        "a11adfbded99484385d25a81bee4cc59e054f98bc1f5792310e56b4eadb8e0fc",
        "63e717f2c581844342b19353d0119764ae8d661471a554e7fa26b2a329aaab07",
        "target_version := lease_record.target_version",
        "target_version := attestation_row.target_version",
        "d714be0f03a6b8a7b1dd0c276214255fb2c2b7d6841987aba5d63819284d6680",
        "4620414e2a07bb0d3421c5480cb25339d3c1e53f018b9997cb153aba7b1aa8db",
    ] {
        assert!(HEARTBEAT_MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        HEARTBEAT_MIGRATION
            .matches("EXECUTE function_definition")
            .count(),
        1
    );
    let lock_fragment = HEARTBEAT_MIGRATION
        .split("new_lock_fragment TEXT :=")
        .nth(1)
        .unwrap()
        .split("old_revision_fragment TEXT :=")
        .next()
        .unwrap();
    assert!(
        lock_fragment.find("pg_advisory_xact_lock_shared").unwrap()
            < lock_fragment.find("SELECT attestation.*").unwrap()
    );
    assert!(!HEARTBEAT_MIGRATION.contains("CREATE FUNCTION public."));
}

#[test]
fn heartbeat_accepts_only_a_fresh_exact_or_single_owner_rollover() {
    for required in [
        "acknowledgement_row.observed_owner_revision",
        "IS DISTINCT FROM owner_row.owner_revision",
        "acknowledgement_row.requested_owner_observed_at",
        "> pg_catalog.clock_timestamp()",
        "acknowledgement_row.requested_owner_expires_at",
        "<= pg_catalog.clock_timestamp()",
        "acknowledgement_row.observed_owner_revision",
        "= owner_row.owner_revision",
        "acknowledgement_row.requested_owner_expires_at",
        "= owner_row.expires_at",
        "owner_row.owner_revision",
        "> acknowledgement_row.observed_owner_revision",
        "- acknowledgement_row.observed_owner_revision = 1",
        "owner_row.expires_at",
        "> acknowledgement_row.requested_owner_expires_at",
        "63e717f2c581844342b19353d0119764ae8d661471a554e7fa26b2a329aaab07",
        "07859d61ceab00eeeaeba860337927b36718d60b2eff468362c3fad57f703327",
    ] {
        assert!(ROLLOVER_MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        ROLLOVER_MIGRATION
            .matches("EXECUTE pg_catalog.replace(")
            .count(),
        1
    );
    assert!(!ROLLOVER_MIGRATION.contains("CREATE FUNCTION public."));
}

#[test]
fn heartbeat_requires_a_post_attestation_acknowledgement() {
    for required in [
        "acknowledgement_row.observed_owner_revision",
        "<= (",
        "attestation_row.v2_route_admission",
        "->> ''attested_owner_revision''",
        ")::BIGINT",
        "07859d61ceab00eeeaeba860337927b36718d60b2eff468362c3fad57f703327",
        "dc83d0fefb3c56affb2e97b58106cc853a71fad787f5ec7a1f548fa07178f1e9",
    ] {
        assert!(
            POST_ATTESTATION_ACK_MIGRATION.contains(required),
            "{required}"
        );
    }
    assert_eq!(
        POST_ATTESTATION_ACK_MIGRATION
            .matches("EXECUTE pg_catalog.replace(")
            .count(),
        1
    );
    assert!(!POST_ATTESTATION_ACK_MIGRATION.contains("CREATE FUNCTION public."));
}

#[test]
fn replacement_is_drift_pinned_and_preserves_function_metadata() {
    for required in [
        "pg_catalog.to_regprocedure(function_identity)",
        "pg_catalog.pg_get_functiondef(function_row.oid)",
        "observed_definition_digest <> old_definition_digest",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_definition_digest <> expected_definition_digest",
        "'oid', function_row.oid::TEXT",
        "'owner', function_row.proowner::TEXT",
        "'acl', pg_catalog.to_jsonb(function_row.proacl)",
        "'language', function_row.prolang::TEXT",
        "'kind', function_row.prokind",
        "'volatile', function_row.provolatile",
        "'strict', function_row.proisstrict",
        "'security_definer', function_row.prosecdef",
        "'parallel', function_row.proparallel",
        "'returns_set', function_row.proretset",
        "'rows', function_row.prorows",
        "'config', pg_catalog.to_jsonb(function_row.proconfig)",
        "'leakproof', function_row.proleakproof",
        "'argument_defaults', function_row.pronargdefaults",
        "'variadic', function_row.provariadic::TEXT",
        "'return_type', function_row.prorettype::TEXT",
    ] {
        for migration in [
            HEARTBEAT_MIGRATION,
            ROLLOVER_MIGRATION,
            POST_ATTESTATION_ACK_MIGRATION,
            POST_ATTESTATION_ACK_MANIFEST_MIGRATION,
            POST_ATTESTATION_ACK_READINESS_MIGRATION,
        ] {
            assert!(migration.contains(required), "{required}");
        }
    }
}

#[test]
fn serving_manifest_and_readiness_are_refreshed_end_to_end() {
    for required in [
        "public.starring_runtime_serving_schema_manifest_v1()",
        "095b56dd1d761868765c6e21aaf49bdbeed86bc2be95218c191acefdd12a6047",
        "57ef9c351e59e2fcd23789dd7386345ce0023469817cae8d3d6547a031b259b5",
        "012e4f8c1dcde470f395360a50e443f80360b868c119b71b612ee20c983801ab",
        "ad683aca71f9271021b9d80293ca99fc307473a78f04373f7dfdfc531bf6adcd",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(MANIFEST_MIGRATION.contains(required), "{required}");
    }
    for required in [
        "public.starring_runtime_serving_database_readiness_v1()",
        "012e4f8c1dcde470f395360a50e443f80360b868c119b71b612ee20c983801ab",
        "ad683aca71f9271021b9d80293ca99fc307473a78f04373f7dfdfc531bf6adcd",
        "16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15",
        "01bd850ba45c6ee7a2edfbe0bae8cdef79f53c219d05ec90549076ae227a6f71",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(READINESS_MIGRATION.contains(required), "{required}");
    }
    for required in [
        "57ef9c351e59e2fcd23789dd7386345ce0023469817cae8d3d6547a031b259b5",
        "9b4dbfd385898e7ed1ce8001ec0c45067fb974e372d8967717089f81285a1783",
        "ad683aca71f9271021b9d80293ca99fc307473a78f04373f7dfdfc531bf6adcd",
        "4704438e7b783e5507f7c0f3fdb8629a37d046a8e5faa5e530e65ad7e8580b7a",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(ROLLOVER_MANIFEST_MIGRATION.contains(required), "{required}");
    }
    for required in [
        "ad683aca71f9271021b9d80293ca99fc307473a78f04373f7dfdfc531bf6adcd",
        "4704438e7b783e5507f7c0f3fdb8629a37d046a8e5faa5e530e65ad7e8580b7a",
        "01bd850ba45c6ee7a2edfbe0bae8cdef79f53c219d05ec90549076ae227a6f71",
        "691b12d79fc52a9a7b817c584bfcac3a25fa2e9f5f841e5954a535405cb3e191",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(
            ROLLOVER_READINESS_MIGRATION.contains(required),
            "{required}"
        );
    }
    for required in [
        "9b4dbfd385898e7ed1ce8001ec0c45067fb974e372d8967717089f81285a1783",
        "11d0780fcc13729aa018acf80b8741c3eb3136f8b68ca42f4b600303389b1eab",
        "4704438e7b783e5507f7c0f3fdb8629a37d046a8e5faa5e530e65ad7e8580b7a",
        "3a11d73fed6a2bd05e932c27c7e2237d568be66777db14c79a44e84a5816e940",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(
            POST_ATTESTATION_ACK_MANIFEST_MIGRATION.contains(required),
            "{required}"
        );
    }
    for required in [
        "4704438e7b783e5507f7c0f3fdb8629a37d046a8e5faa5e530e65ad7e8580b7a",
        "3a11d73fed6a2bd05e932c27c7e2237d568be66777db14c79a44e84a5816e940",
        "691b12d79fc52a9a7b817c584bfcac3a25fa2e9f5f841e5954a535405cb3e191",
        "e2e2cbbecc245e4c8d96b264d5bf89f1ce01cf4613c86f2d954bbdeeb3d2ad8a",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(
            POST_ATTESTATION_ACK_READINESS_MIGRATION.contains(required),
            "{required}"
        );
    }
    assert!(SERVING_DATABASE
        .contains("e598fb40785ccd66ce44ec6c7f85e52fd9e004ab1e05de9c0c03963f06df45f1"));
}

#[test]
fn migrations_are_bounded_and_comment_free() {
    for migration in [
        HEARTBEAT_MIGRATION,
        MANIFEST_MIGRATION,
        READINESS_MIGRATION,
        ROLLOVER_MIGRATION,
        ROLLOVER_MANIFEST_MIGRATION,
        ROLLOVER_READINESS_MIGRATION,
        POST_ATTESTATION_ACK_MIGRATION,
        POST_ATTESTATION_ACK_MANIFEST_MIGRATION,
        POST_ATTESTATION_ACK_READINESS_MIGRATION,
    ] {
        assert!(migration.contains("SET LOCAL lock_timeout = '5s'"));
        assert!(migration.contains("SET LOCAL statement_timeout = '30s'"));
        assert!(migration
            .ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;\n"));
        assert!(!migration.contains("--"));
        assert!(!migration.contains("/*"));
        assert!(!migration.contains("//"));
    }
}
