const MIGRATION: &str =
    include_str!("../../../migrations/202607310008_refresh_cross_capability_readiness.sql");
const PREVIOUS_READINESS_MIGRATION: &str =
    include_str!("../../../migrations/202607310003_refresh_solo_approval_readiness_pins.sql");
const EXACT_TARGET_DATABASE_SOURCE: &str =
    include_str!("../../automation-runtime-convergence-postgres/src/hydration/database.rs");
const SERVING_DATABASE_SOURCE: &str =
    include_str!("../../automation-runtime-serving-postgres/src/database.rs");

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
fn cross_capability_refresh_is_forward_only_bounded_and_comment_free() {
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET lock_timeout;\n"));
    assert!(MIGRATION.contains("starring-runtime-writer-fence-v1"));
    assert!(MIGRATION.contains("LOCK TABLE public.runtime_deployments IN ACCESS SHARE MODE;"));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    for forbidden in [
        "GRANT ",
        "REVOKE ",
        "DROP ",
        "DELETE FROM",
        "UPDATE public.",
        "INSERT INTO public.",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn exact_target_manifest_chain_is_pinned_end_to_end() {
    for required in [
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "d5f52b36ec0e5002d3330ae242e31e6706cab19405c7541ab8cc4a5244637783",
        "4f0faaa39110eabbdfb432ff7437776adb1044911f6e3fe4aad64e529a4fa02a",
        "c8e5559234a54c8b4b3be342a98badc0f63d3fb4ae59beea50d105938730ec7d",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "aee90f2f78d8106e298c8075b0710bca6d47b3b37cc9d2c6598a4f9f769b9f7d",
        "3f6a6a99409f21b6d1af71ecd87f86024b0a4f0c939f1d6dbea558d9991e7612",
        "public.starring_runtime_exact_target_database_readiness_v2()",
        "b42afbec22a0531a64708ad8bb3c7f26d73609bd4f0e1e80ec6d0602e98cc966",
        "3ada22bd8ca9b0eec6528ec9f6bff320c9bf29d816ee00d24a9cdec592aa359b",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    let postflight = dollar_block("postflight");
    assert!(postflight.contains("public.starring_runtime_exact_target_schema_manifest_v1()"));
    assert!(postflight.contains("public.starring_runtime_exact_target_schema_manifest_v2()"));
}

#[test]
fn serving_manifest_chain_is_pinned_end_to_end() {
    for required in [
        "public.starring_runtime_serving_schema_manifest_v1()",
        "723aff77059617f7c7a2d7c7d95f685f3b546527b3e73f2b05fa280bc3db7bed",
        "095b56dd1d761868765c6e21aaf49bdbeed86bc2be95218c191acefdd12a6047",
        "012e4f8c1dcde470f395360a50e443f80360b868c119b71b612ee20c983801ab",
        "public.starring_runtime_serving_database_readiness_v1()",
        "644b932f77a787089cb71a273be7e56c5e226f06287b5f6a23a0c4d9bbcff762",
        "16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(
        dollar_block("postflight").contains("public.starring_runtime_serving_schema_manifest_v1()")
    );
}

#[test]
fn readiness_metadata_and_rust_pins_are_exact() {
    for required in [
        "metadata_after IS DISTINCT FROM metadata_before",
        "expected_definition_digests[function_index]",
        "'oid', function_row.oid::TEXT",
        "'owner', function_row.proowner::TEXT",
        "'acl', pg_catalog.to_jsonb(function_row.proacl)",
        "'security_definer', function_row.prosecdef",
        "'return_type', function_row.prorettype::TEXT",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(PREVIOUS_READINESS_MIGRATION
        .contains("2b28b5bac9a444333d1681ccc158243d8a0d010818fa0719374699f8d0275c43"));
    assert!(PREVIOUS_READINESS_MIGRATION
        .contains("977410de87917e582c6018c0ddcea164045b82a4550fb166ff138e3efa65238d"));
    assert!(EXACT_TARGET_DATABASE_SOURCE
        .contains("3ada22bd8ca9b0eec6528ec9f6bff320c9bf29d816ee00d24a9cdec592aa359b"));
    assert!(SERVING_DATABASE_SOURCE
        .contains("16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15"));
}
