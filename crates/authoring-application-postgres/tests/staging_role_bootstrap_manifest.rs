use std::collections::BTreeSet;

#[path = "support/staging_api_capabilities.rs"]
mod staging_api_capabilities;

use staging_api_capabilities::CAPABILITIES;

const BOOTSTRAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-api-role-bootstrap.sql"
));
const ENABLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-api-role-enable.sql"
));

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

fn expected_roles() -> BTreeSet<String> {
    CAPABILITIES
        .iter()
        .map(|capability| capability.staging_role.to_string())
        .collect()
}

fn expected_capabilities() -> BTreeSet<(String, String)> {
    CAPABILITIES
        .iter()
        .flat_map(|capability| {
            capability
                .functions
                .iter()
                .map(|function| (capability.staging_role.to_string(), (*function).to_string()))
        })
        .collect()
}

fn manifest_roles(source: &str, next: &str) -> Vec<String> {
    tuple_rows(values_section(
        source,
        "INSERT INTO pg_temp.starring_api_request_roles",
        next,
    ))
    .into_iter()
    .map(|fields| {
        assert_eq!(fields.len(), 1);
        fields.into_iter().next().unwrap()
    })
    .collect()
}

fn manifest_capabilities(source: &str, next: &str) -> Vec<(String, String)> {
    tuple_rows(values_section(
        source,
        "INSERT INTO pg_temp.starring_api_capability_manifest",
        next,
    ))
    .into_iter()
    .map(|fields| {
        assert_eq!(fields.len(), 2);
        (fields[0].clone(), fields[1].clone())
    })
    .collect()
}

fn assert_exact_set<T>(label: &str, rows: Vec<T>, expected: &BTreeSet<T>) -> BTreeSet<T>
where
    T: Clone + Ord + std::fmt::Debug,
{
    let actual = rows.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        rows.len(),
        actual.len(),
        "{label} contains duplicate entries"
    );
    let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
    assert!(missing.is_empty(), "{label} is missing {missing:?}");
    let extra = actual.difference(expected).cloned().collect::<Vec<_>>();
    assert!(extra.is_empty(), "{label} contains extra entries {extra:?}");
    actual
}

fn assert_cardinality_guard(source: &str, expected: usize) {
    let guard = format!(
        "OR (SELECT pg_catalog.count(*) FROM pg_temp.starring_api_capability_manifest) <> {expected}"
    );
    assert_eq!(source.matches(&guard).count(), 1);
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
        assert!(
            BOOTSTRAP.contains(required),
            "missing bootstrap guard: {required}"
        );
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
fn staging_role_bootstrap_matches_the_fourteen_readiness_allowlists() {
    let fixtures = CAPABILITIES
        .iter()
        .map(|capability| capability.fixture_label)
        .collect::<BTreeSet<_>>();
    assert_eq!(fixtures.len(), 14);

    let expected_roles = expected_roles();
    assert_eq!(expected_roles.len(), 14);
    let bootstrap_roles = assert_exact_set(
        "bootstrap roles",
        manifest_roles(
            BOOTSTRAP,
            "CREATE TEMP TABLE starring_api_capability_manifest",
        ),
        &expected_roles,
    );
    let enable_roles = assert_exact_set(
        "enable roles",
        manifest_roles(ENABLE, "CREATE TEMP TABLE starring_api_capability_manifest"),
        &expected_roles,
    );
    assert_eq!(bootstrap_roles, enable_roles);

    let expected_capabilities = expected_capabilities();
    assert_eq!(expected_capabilities.len(), 48);
    let bootstrap_capabilities = assert_exact_set(
        "bootstrap capabilities",
        manifest_capabilities(BOOTSTRAP, "DO $roles$"),
        &expected_capabilities,
    );
    let enable_capabilities = assert_exact_set(
        "enable capabilities",
        manifest_capabilities(ENABLE, "DO $preflight$"),
        &expected_capabilities,
    );
    assert_eq!(bootstrap_capabilities, enable_capabilities);
    assert_cardinality_guard(BOOTSTRAP, expected_capabilities.len());
    assert_cardinality_guard(ENABLE, expected_capabilities.len());
}
