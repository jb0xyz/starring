use std::collections::{BTreeMap, BTreeSet};

const BOOTSTRAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-runtime-role-bootstrap.sql"
));
const API_BOOTSTRAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-api-role-bootstrap.sql"
));
const INTEGRATED_HBA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-integrated-pg_hba.conf"
));
const INTEGRATED_BOOTSTRAP_HBA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-integrated-bootstrap-pg_hba.conf"
));
const INTEGRATED_BOOTSTRAP_IDENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-integrated-bootstrap-pg_ident.conf"
));
const RUNBOOK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/superpowers/runbooks/2026-07-29-macos-starring-runtime-staging-operations.md"
));
const RUNTIME_HBA_ROLES: &str = "starring_runtime_execution,starring_runtime_exact_target,starring_runtime_panel,starring_runtime_serving,starring_runtime_interaction";
const API_HBA_ROLES: &str = "starring_identity_oauth,starring_identity_issuer,starring_identity_session,starring_identity_security,starring_installation_authority_reader,starring_authorized_snapshot_reader,starring_promotion_executor,starring_decision_reader,starring_decision_approval,starring_decision_rejection,starring_decision_apply,starring_decision_cancellation,starring_deployment_status_reader,starring_operational_deployment_status_reader";

fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap();
    let remaining = &source[start..];
    let end = remaining.find(end).unwrap();
    &remaining[..end]
}

fn tuple_rows(section: &str) -> Vec<(String, String)> {
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
                .unwrap()
                .strip_suffix('\'')
                .unwrap();
            let (left, right) = row.split_once("', '").unwrap();
            Some((left.to_owned(), right.to_owned()))
        })
        .collect()
}

fn position(source: &str, needle: &str) -> usize {
    source.find(needle).unwrap()
}

fn single_column_tuple_rows(section: &str) -> BTreeSet<String> {
    section
        .lines()
        .filter_map(|line| {
            let row = line.trim();
            if !row.starts_with("('") {
                return None;
            }
            Some(
                row.strip_prefix("('")
                    .unwrap()
                    .strip_suffix("'),")
                    .or_else(|| row.strip_prefix("('").unwrap().strip_suffix("');"))
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect()
}

fn runtime_bootstrap_default_roles() -> BTreeSet<String> {
    BOOTSTRAP
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next() != Some("\\set") {
                return None;
            }
            let variable = fields.next()?;
            let role = fields.next()?;
            if fields.next().is_some()
                || !variable.starts_with("runtime_")
                || !variable.ends_with("_role")
            {
                return None;
            }
            Some(role.to_owned())
        })
        .collect()
}

#[test]
fn staging_runtime_role_bootstrap_pins_the_four_phase_containment_order() {
    assert!(BOOTSTRAP.trim_start().starts_with("\\set ON_ERROR_STOP on"));
    assert_eq!(
        BOOTSTRAP
            .lines()
            .filter(|line| line.trim() == "BEGIN;")
            .count(),
        4
    );
    assert_eq!(
        BOOTSTRAP
            .lines()
            .filter(|line| line.trim() == "COMMIT;")
            .count(),
        4
    );

    let guard = position(BOOTSTRAP, "DO $guard$");
    let seal = position(BOOTSTRAP, "DO $seal$");
    let membership = position(BOOTSTRAP, "DO $membership_cleanup$");
    let isolation = position(BOOTSTRAP, "DO $isolation_guard$");
    let cleanup = position(BOOTSTRAP, "DO $quarantine_cleanup$");
    let postflight = position(BOOTSTRAP, "DO $postflight$");
    let isolation_postflight = position(BOOTSTRAP, "DO $isolation_postflight$");
    let activate = position(BOOTSTRAP, "DO $activate$");
    let activation_postflight = position(BOOTSTRAP, "DO $activation_postflight$");

    assert!(guard < seal);
    assert!(seal < membership);
    assert!(membership < isolation);
    assert!(isolation < cleanup);
    assert!(cleanup < postflight);
    assert!(postflight < isolation_postflight);
    assert!(isolation_postflight < activate);
    assert!(activate < activation_postflight);
    assert_eq!(BOOTSTRAP.matches("ALTER ROLE %I LOGIN").count(), 1);
    assert!(position(BOOTSTRAP, "ALTER ROLE %I LOGIN") > isolation_postflight);
    assert_eq!(
        BOOTSTRAP
            .matches("activity.backend_type = 'client backend'")
            .count(),
        2
    );
    assert_eq!(BOOTSTRAP.matches("activity.usesysid IN").count(), 2);

    let seal_transaction = section(BOOTSTRAP, "DO $seal$", "COMMIT;");
    assert!(seal_transaction.contains("NOLOGIN NOSUPERUSER"));
    assert!(seal_transaction.contains("PASSWORD NULL"));
    assert!(!seal_transaction.contains("pg_auth_members"));

    let membership_transaction = section(BOOTSTRAP, "DO $membership_cleanup$", "COMMIT;");
    assert!(membership_transaction.contains("pg_catalog.pg_auth_members"));
    assert!(membership_transaction.contains("SET LOCAL ROLE %I"));
    assert!(membership_transaction.contains("REVOKE %I FROM %I CASCADE"));
    assert!(membership_transaction.contains("EXECUTE 'RESET ROLE'"));

    let default_owner_selection = section(BOOTSTRAP, "FOR owner_entry IN", "LOOP");
    assert!(default_owner_selection.contains("role.oid NOT IN"));
    assert!(default_owner_selection.contains("pg_temp.starring_runtime_capability_roles"));

    for required in [
        "starring-runtime-dedicated-staging-cluster-v2:%s:%s:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation",
        "current_setting('server_version_num')",
        "NOT BETWEEN 160000 AND 169999",
        "activity.backend_type = 'client backend'",
        "pg_catalog.pg_prepared_xacts",
        "pg_catalog.pg_init_privs",
        "pg_catalog.left(namespace.nspname, 3) = 'pg_'",
        "pg_catalog.pg_my_temp_schema()",
        "_pg_foreign_data_wrappers",
        "_pg_foreign_servers",
        "_pg_foreign_table_columns",
        "_pg_foreign_tables",
        "_pg_user_mappings",
        "sql_parts",
        "transforms",
        "runtime system PUBLIC privileges are invalid",
        "runtime public boundary privileges are invalid",
        "runtime capability function ACL topology is invalid",
        "REVOKE ALL PRIVILEGES ON ROUTINE %s FROM %s CASCADE",
        "ALTER ROLE %I LOGIN",
    ] {
        assert!(
            BOOTSTRAP.contains(required),
            "missing runtime bootstrap contract: {required}"
        );
    }

    let upper = BOOTSTRAP.to_ascii_uppercase();
    assert!(!upper.contains("PASSWORD '"));
    assert!(!upper.contains("ENCRYPTED PASSWORD"));
    assert!(!BOOTSTRAP.contains("postgres://"));
    assert!(!BOOTSTRAP.contains("postgresql://"));
    assert!(!BOOTSTRAP.contains("cfat_"));
    assert!(!BOOTSTRAP.contains("cfut_"));
    assert!(!BOOTSTRAP.contains("/*"));
    assert!(!BOOTSTRAP.contains("*/"));
    assert!(!BOOTSTRAP
        .lines()
        .any(|line| line.trim_start().starts_with("--")));
}

#[test]
fn staging_runtime_capability_manifest_is_exact_and_unique() {
    let role_section = section(
        BOOTSTRAP,
        "INSERT INTO pg_temp.starring_runtime_capability_roles",
        "CREATE TEMP TABLE starring_runtime_capability_functions",
    );
    for (capability, variable, role) in [
        (
            "execution",
            "runtime_execution_role",
            "starring_runtime_execution",
        ),
        (
            "exact_target",
            "runtime_exact_target_role",
            "starring_runtime_exact_target",
        ),
        ("panel", "runtime_panel_role", "starring_runtime_panel"),
        (
            "serving",
            "runtime_serving_role",
            "starring_runtime_serving",
        ),
        (
            "interaction",
            "runtime_interaction_role",
            "starring_runtime_interaction",
        ),
    ] {
        assert!(BOOTSTRAP.contains(&format!("\\set {variable} {role}")));
        assert!(role_section.contains(&format!("('{capability}', :'{variable}')")));
        assert!(BOOTSTRAP.contains(&format!("WHEN '{capability}' THEN '{role}'")));
    }
    assert!(BOOTSTRAP.contains("runtime capability role identities are not exact"));

    let functions = tuple_rows(section(
        BOOTSTRAP,
        "INSERT INTO pg_temp.starring_runtime_capability_functions",
        "SELECT pg_catalog.pg_advisory_lock",
    ));
    assert_eq!(functions.len(), 49);

    let mut identities = BTreeSet::new();
    let mut counts = BTreeMap::new();
    for (capability, identity) in functions {
        assert!(identities.insert(identity));
        *counts.entry(capability).or_insert(0_usize) += 1;
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            ("exact_target".to_owned(), 3),
            ("execution".to_owned(), 28),
            ("interaction".to_owned(), 5),
            ("panel".to_owned(), 9),
            ("serving".to_owned(), 4),
        ])
    );
}

#[test]
fn staging_runtime_runbook_requires_cluster_wide_fail_closed_rotation() {
    for required in [
        "starring-runtime-dedicated-staging-cluster-v2:",
        "cluster-wide-public-acl-reset",
        "bidirectional-runtime-membership-revocation",
        "api_was_loaded=true",
        "local.starring.api.staging",
        "client backend",
        "prepared",
        "NOLOGIN",
        "immediate",
        "quarantine",
        "/health/live",
        "/health/ready",
        "Do not silently",
        "30-second",
        "kickstart -p",
    ] {
        assert!(
            RUNBOOK.contains(required),
            "missing runtime runbook contract: {required}"
        );
    }
}

#[test]
fn integrated_staging_hba_pins_the_exact_fail_closed_boundary() {
    let expected = vec![
        format!(
            "hostnossl starring_runtime_staging {RUNTIME_HBA_ROLES} 127.0.0.1/32 scram-sha-256"
        ),
        format!("host all {RUNTIME_HBA_ROLES} 0.0.0.0/0 reject"),
        format!("host all {RUNTIME_HBA_ROLES} ::0/0 reject"),
        format!("local all {RUNTIME_HBA_ROLES} reject"),
        format!("hostnossl starring_runtime_staging {API_HBA_ROLES} 127.0.0.1/32 scram-sha-256"),
        format!("host all {API_HBA_ROLES} 0.0.0.0/0 reject"),
        format!("host all {API_HBA_ROLES} ::0/0 reject"),
        format!("local all {API_HBA_ROLES} reject"),
        "hostnossl postgres,starring_runtime_staging starring_cluster_admin 127.0.0.1/32 scram-sha-256".to_owned(),
        "host all all 0.0.0.0/0 reject".to_owned(),
        "host all all ::0/0 reject".to_owned(),
        "local all all reject".to_owned(),
        "host replication all 0.0.0.0/0 reject".to_owned(),
        "host replication all ::0/0 reject".to_owned(),
        "local replication all reject".to_owned(),
    ];
    let observed = INTEGRATED_HBA
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(observed, expected);
    assert!(INTEGRATED_HBA.ends_with('\n'));
    assert!(!INTEGRATED_HBA.contains('\r'));
    assert!(!INTEGRATED_HBA.contains('#'));

    let runtime_roles = RUNTIME_HBA_ROLES
        .split(',')
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        runtime_roles,
        BTreeSet::from([
            "starring_runtime_exact_target".to_owned(),
            "starring_runtime_execution".to_owned(),
            "starring_runtime_interaction".to_owned(),
            "starring_runtime_panel".to_owned(),
            "starring_runtime_serving".to_owned(),
        ])
    );
    assert_eq!(runtime_roles, runtime_bootstrap_default_roles());

    let api_roles = API_HBA_ROLES
        .split(',')
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let api_manifest_roles = single_column_tuple_rows(section(
        API_BOOTSTRAP,
        "INSERT INTO pg_temp.starring_api_request_roles",
        "CREATE TEMP TABLE starring_api_capability_manifest",
    ));
    assert_eq!(api_roles, api_manifest_roles);
    assert_eq!(api_roles.len(), 14);
    assert!(runtime_roles.is_disjoint(&api_roles));

    let lower = INTEGRATED_HBA.to_ascii_lowercase();
    for forbidden in [
        "password",
        "secret",
        "token",
        "postgres://",
        "postgresql://",
        "keychain:",
        "cfat_",
        "cfut_",
        "trust",
        "peer",
        "md5",
        "include",
        "@",
    ] {
        assert!(!lower.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn integrated_staging_bootstrap_authentication_is_temporary_and_exact() {
    assert_eq!(
        INTEGRATED_BOOTSTRAP_HBA.lines().collect::<Vec<_>>(),
        vec![
            "local postgres,starring_runtime_staging starring_cluster_admin peer map=starring_bootstrap",
            "host all all 0.0.0.0/0 reject",
            "host all all ::0/0 reject",
            "local all all reject",
            "host replication all 0.0.0.0/0 reject",
            "host replication all ::0/0 reject",
            "local replication all reject",
        ]
    );
    assert_eq!(
        INTEGRATED_BOOTSTRAP_IDENT,
        "starring_bootstrap jungbogeon starring_cluster_admin\n"
    );
    for source in [INTEGRATED_BOOTSTRAP_HBA, INTEGRATED_BOOTSTRAP_IDENT] {
        assert!(source.ends_with('\n'));
        assert!(!source.contains('\r'));
        assert!(!source.contains('#'));
        let lower = source.to_ascii_lowercase();
        for forbidden in [
            "password",
            "secret",
            "token",
            "postgres://",
            "postgresql://",
            "keychain:",
            "cfat_",
            "cfut_",
            "trust",
            "md5",
            "include",
            "@",
        ] {
            assert!(!lower.contains(forbidden), "{forbidden}");
        }
    }
    assert!(!INTEGRATED_HBA.contains("peer"));
    assert!(!INTEGRATED_HBA.contains("starring_bootstrap"));
}
