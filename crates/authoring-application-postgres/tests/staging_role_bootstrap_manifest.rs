use std::collections::BTreeMap;

const BOOTSTRAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-api-role-bootstrap.sql"
));
const ENABLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-api-role-enable.sql"
));
const API_READINESS_TEST: &str = include_str!("postgres_product_api_readiness.rs");

const EXPECTED_CAPABILITIES: [(&str, &str); 43] = [
    (
        "starring_identity_oauth",
        "public.starring_product_oauth_database_identity_v1()",
    ),
    (
        "starring_identity_oauth",
        "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
    ),
    (
        "starring_identity_oauth",
        "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
    ),
    (
        "starring_identity_issuer",
        "public.starring_product_session_issuer_database_identity_v1()",
    ),
    (
        "starring_identity_issuer",
        "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
    ),
    (
        "starring_identity_session",
        "public.starring_product_session_api_database_identity_v1()",
    ),
    (
        "starring_identity_session",
        "public.starring_product_session_read_v1(bytea)",
    ),
    (
        "starring_identity_session",
        "public.starring_product_session_mutation_read_v1(bytea)",
    ),
    (
        "starring_identity_session",
        "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
    ),
    (
        "starring_identity_session",
        "public.starring_product_session_logout_read_v1(bytea)",
    ),
    (
        "starring_identity_session",
        "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
    ),
    (
        "starring_identity_security",
        "public.starring_product_security_revoker_database_identity_v1()",
    ),
    (
        "starring_identity_security",
        "public.starring_product_session_security_revoke_v1(bytea)",
    ),
    (
        "starring_installation_authority_reader",
        "public.starring_product_installation_authority_reader_database_identity_v1()",
    ),
    (
        "starring_installation_authority_reader",
        "public.starring_product_installation_authority_read_v1(text,text,bytea)",
    ),
    (
        "starring_authorized_snapshot_reader",
        "public.starring_product_authorized_snapshot_reader_database_identity_v1()",
    ),
    (
        "starring_authorized_snapshot_reader",
        "public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)",
    ),
    (
        "starring_authorized_snapshot_reader",
        "public.starring_product_authorized_snapshot_key_coverage_v1(text[])",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_executor_database_identity_v1()",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    ),
    (
        "starring_promotion_executor",
        "public.starring_product_promotion_keyring_coverage_v1(text[],text[])",
    ),
    (
        "starring_decision_reader",
        "public.starring_product_decision_reader_database_identity_v1()",
    ),
    (
        "starring_decision_reader",
        "public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)",
    ),
    (
        "starring_decision_approval",
        "public.starring_product_approval_executor_database_identity_v1()",
    ),
    (
        "starring_decision_approval",
        "public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)",
    ),
    (
        "starring_decision_approval",
        "public.starring_product_approval_keyring_coverage_v1(text[],text[])",
    ),
    (
        "starring_decision_rejection",
        "public.starring_product_rejection_executor_database_identity_v1()",
    ),
    (
        "starring_decision_rejection",
        "public.starring_product_rejection_keyring_coverage_v1(text[],text[])",
    ),
    (
        "starring_decision_rejection",
        "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)",
    ),
    (
        "starring_decision_apply",
        "public.starring_product_apply_executor_database_identity_v1()",
    ),
    (
        "starring_decision_apply",
        "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    ),
    (
        "starring_decision_apply",
        "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)",
    ),
    (
        "starring_decision_apply",
        "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
    ),
    (
        "starring_decision_apply",
        "public.starring_product_apply_keyring_coverage_v1(text[],text[])",
    ),
    (
        "starring_deployment_status_reader",
        "public.starring_product_deployment_status_reader_database_identity_v1()",
    ),
    (
        "starring_deployment_status_reader",
        "public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)",
    ),
    (
        "starring_operational_deployment_status_reader",
        "public.starring_product_deployment_status_reader_database_identity_v2()",
    ),
    (
        "starring_operational_deployment_status_reader",
        "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)",
    ),
];

fn values_section<'a>(source: &'a str, insertion: &str, next: &str) -> &'a str {
    let start = source.find(insertion).unwrap();
    let remaining = &source[start..];
    let end = remaining.find(next).unwrap();
    &remaining[..end]
}

fn tuple_rows(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .filter_map(|line| {
            let row = line.trim();
            if !row.starts_with("('") {
                return None;
            }
            let row = row
                .strip_prefix("('")
                .unwrap()
                .strip_suffix("),")
                .or_else(|| row.strip_prefix("('").unwrap().strip_suffix(");"))
                .unwrap();
            let row = row.strip_suffix('\'').unwrap();
            Some(row.split("', '").map(ToString::to_string).collect())
        })
        .collect()
}

#[test]
fn staging_role_bootstrap_is_atomic_bounded_and_secret_free() {
    assert!(BOOTSTRAP.trim_start().starts_with("BEGIN;"));
    assert!(BOOTSTRAP.trim_end().ends_with("COMMIT;"));
    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '60s';",
        "SET LOCAL idle_in_transaction_session_timeout = '60s';",
        "SET LOCAL search_path = pg_catalog;",
        "starring.expected_staging_database",
        "starring.expected_staging_system_identifier",
        "pg_catalog.pg_control_system()",
        "^starring(_[a-z0-9]+)*_staging(_[a-z0-9]+)*$",
        "staging cluster acknowledgement is invalid",
        "staging role bootstrap requires a cluster administrator",
        "CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL ''infinity'' PASSWORD NULL",
        "ALTER ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL ''infinity'' PASSWORD NULL",
        "ALTER ROLE %I RESET ALL",
        "ALTER ROLE %I IN DATABASE %I RESET ALL",
        "pg_catalog.pg_db_role_setting",
        "staging role setting postflight failed",
        "role.rolvaliduntil",
        "IS DISTINCT FROM 'infinity'::TIMESTAMP WITH TIME ZONE",
        "role.rolpassword IS NOT NULL",
        "REVOKE %I FROM %I GRANTED BY %I CASCADE",
        "pg_catalog.pg_parameter_acl",
        "REVOKE %s ON PARAMETER %I FROM %s GRANTED BY %I CASCADE",
        "SET LOCAL ROLE %I",
        "EXECUTE 'RESET ROLE'",
        "REVOKE ALL PRIVILEGES ON DATABASE %I FROM %s",
        "REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA %I FROM %s CASCADE",
        "REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA %I FROM %s CASCADE",
        "REVOKE ALL PRIVILEGES ON ALL ROUTINES IN SCHEMA %I FROM %s CASCADE",
        "REVOKE %s (%I) ON TABLE %I.%I FROM %s CASCADE",
        "starring_api_default_owners",
        "ALTER DEFAULT PRIVILEGES FOR ROLE %I%s REVOKE ALL PRIVILEGES ON %s FROM %s",
        "ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON TYPES FROM PUBLIC",
        "GRANT CONNECT ON DATABASE %s TO %I",
        "GRANT USAGE ON SCHEMA public TO %I",
        "GRANT EXECUTE ON FUNCTION %s TO %I",
        "EXECUTE WITH GRANT OPTION",
        "staging function ACL postflight failed",
        "staging public database or schema ACL postflight failed",
        "legacy staging API role postflight failed",
        "pg_catalog.pg_shdepend",
        "pg_catalog.pg_terminate_backend(activity.pid, 5000)",
    ] {
        assert!(BOOTSTRAP.contains(required), "missing bootstrap guard: {required}");
    }
    let upper = BOOTSTRAP.to_ascii_uppercase();
    assert!(!upper.contains("CREATE ROLE STARRING_OWNER"));
    assert!(upper.contains("PASSWORD NULL"));
    assert!(!upper.contains("PASSWORD '"));
    assert!(!upper.contains("ENCRYPTED PASSWORD"));
    assert!(!upper.contains("SECRET"));
    assert!(!BOOTSTRAP.contains("postgres://"));
    assert!(!BOOTSTRAP.contains("postgresql://"));
    assert!(!BOOTSTRAP.contains("/*"));
    assert!(!BOOTSTRAP.contains("*/"));
    assert!(!BOOTSTRAP
        .lines()
        .any(|line| line.trim_start().starts_with("--")));
    assert!(ENABLE.trim_start().starts_with("BEGIN;"));
    assert!(ENABLE.trim_end().ends_with("COMMIT;"));
    assert!(ENABLE.contains("role.rolpassword IS NULL"));
    assert!(ENABLE.contains("role.rolpassword NOT LIKE 'SCRAM-SHA-256$%'"));
    assert!(ENABLE.contains("ALTER ROLE %I LOGIN"));
    assert!(ENABLE.contains("pg_catalog.pg_shdepend"));
    assert!(ENABLE.contains("pg_catalog.pg_parameter_acl"));
    assert!(ENABLE.contains("staging owner role enable preflight failed"));
    assert!(ENABLE.contains("managed_default_owners"));
    assert!(ENABLE.contains("staging public database or schema ACL enable preflight failed"));
    assert!(!ENABLE.contains("PASSWORD NULL"));
    assert!(!ENABLE.contains("postgres://"));
    assert!(!ENABLE.contains("postgresql://"));
    assert!(!ENABLE.contains("/*"));
    assert!(!ENABLE.contains("*/"));
    assert!(!ENABLE
        .lines()
        .any(|line| line.trim_start().starts_with("--")));
}

#[test]
fn staging_role_bootstrap_matches_the_thirteen_readiness_allowlists() {
    let role_section = values_section(
        BOOTSTRAP,
        "INSERT INTO pg_temp.starring_api_request_roles",
        "CREATE TEMP TABLE starring_api_capability_manifest",
    );
    let mut roles = tuple_rows(role_section)
        .into_iter()
        .map(|fields| {
            assert_eq!(fields.len(), 1);
            fields.into_iter().next().unwrap()
        })
        .collect::<Vec<_>>();
    roles.sort();
    let expected_counts = EXPECTED_CAPABILITIES.iter().fold(
        BTreeMap::<&str, usize>::new(),
        |mut counts, (role, _)| {
            *counts.entry(role).or_default() += 1;
            counts
        },
    );
    assert_eq!(roles.len(), 13);
    assert_eq!(
        roles,
        expected_counts
            .keys()
            .copied()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );

    let enable_role_section = values_section(
        ENABLE,
        "INSERT INTO pg_temp.starring_api_request_roles",
        "CREATE TEMP TABLE starring_api_capability_manifest",
    );
    let mut enable_roles = tuple_rows(enable_role_section)
        .into_iter()
        .map(|fields| {
            assert_eq!(fields.len(), 1);
            fields.into_iter().next().unwrap()
        })
        .collect::<Vec<_>>();
    enable_roles.sort();
    assert_eq!(enable_roles, roles);

    let capability_section = values_section(
        BOOTSTRAP,
        "INSERT INTO pg_temp.starring_api_capability_manifest",
        "DO $roles$",
    );
    let actual = tuple_rows(capability_section)
        .into_iter()
        .map(|fields| {
            assert_eq!(fields.len(), 2);
            (fields[0].clone(), fields[1].clone())
        })
        .collect::<Vec<_>>();
    let expected = EXPECTED_CAPABILITIES
        .iter()
        .map(|(role, function)| ((*role).to_string(), (*function).to_string()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 43);

    let enable_capability_section = values_section(
        ENABLE,
        "INSERT INTO pg_temp.starring_api_capability_manifest",
        "DO $preflight$",
    );
    let enable_actual = tuple_rows(enable_capability_section)
        .into_iter()
        .map(|fields| {
            assert_eq!(fields.len(), 2);
            (fields[0].clone(), fields[1].clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(enable_actual, expected);

    for (role, function) in EXPECTED_CAPABILITIES {
        assert!(API_READINESS_TEST.contains(function));
        assert_eq!(
            actual
                .iter()
                .filter(|(actual_role, actual_function)| {
                    actual_role == role && actual_function == function
                })
                .count(),
            1
        );
    }
}
