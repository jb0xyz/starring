use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};

use crate::identity::{
    exact_admin_connect_options, exact_admin_target_connect_options,
    exact_database_connect_options, APPLICATION_DATABASE_IDENTITIES, CLUSTER_ADMIN_ROLE,
    DATABASE_HOST, DATABASE_NAME, PRODUCT_ACTION_KEYRING_IDENTITY, RUNTIME_KEYCHAIN_SERVICE,
    SNAPSHOT_ENVELOPE_KEYRING_IDENTITY,
};
use crate::keychain::KeychainClientV1;
use crate::keyring::validate_keyring_pair;
use crate::{ProvisionerErrorV1, StagingAcknowledgementV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalVerificationReportV1 {
    application_database_credentials: usize,
    hba_rules: usize,
    keyrings: usize,
}

impl FinalVerificationReportV1 {
    pub const fn database(self) -> &'static str {
        DATABASE_NAME
    }

    pub const fn application_database_credentials(self) -> usize {
        self.application_database_credentials
    }

    pub const fn hba_rules(self) -> usize {
        self.hba_rules
    }

    pub const fn keyrings(self) -> usize {
        self.keyrings
    }
}

pub async fn verify_final(
    acknowledgement: StagingAcknowledgementV1,
) -> Result<FinalVerificationReportV1, ProvisionerErrorV1> {
    let keychain = KeychainClientV1::new()?;
    keychain.preflight_discord()?;
    let product_action_keyring = keychain.read_required(PRODUCT_ACTION_KEYRING_IDENTITY)?;
    let snapshot_envelope_keyring = keychain.read_required(SNAPSHOT_ENVELOPE_KEYRING_IDENTITY)?;
    validate_keyring_pair(&product_action_keyring, &snapshot_envelope_keyring)?;
    let admin_value = keychain.read_required(crate::identity::ADMIN_KEYCHAIN_IDENTITY)?;
    let admin_options = exact_admin_connect_options(&admin_value)?;
    let mut admin = PgConnection::connect_with(&admin_options)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseConnection)?;
    verify_admin_connection(&mut admin, acknowledgement.system_identifier()).await?;
    verify_final_roles(&mut admin).await?;
    verify_final_hba(&mut admin).await?;
    drop(admin);
    let admin_target_options = exact_admin_target_connect_options(&admin_value)?;
    let mut admin_target = PgConnection::connect_with(&admin_target_options)
        .await
        .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
    verify_admin_target_connection(&mut admin_target).await?;
    drop(admin_target);
    for identity in APPLICATION_DATABASE_IDENTITIES {
        let value = keychain.read_required(crate::identity::KeychainIdentityV1 {
            service: identity.service,
            account: identity.account,
        })?;
        let options = exact_database_connect_options(&value, identity)?;
        let mut connection = PgConnection::connect_with(&options)
            .await
            .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
        verify_application_connection(&mut connection, identity.role).await?;
    }
    Ok(FinalVerificationReportV1 {
        application_database_credentials: APPLICATION_DATABASE_IDENTITIES.len(),
        hba_rules: expected_hba_rules().len(),
        keyrings: 2,
    })
}

pub(crate) async fn verify_admin_connection(
    connection: &mut PgConnection,
    system_identifier: &str,
) -> Result<(), ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT pg_catalog.current_setting('server_version_num')::INTEGER, current_user = session_user AND current_user = 'starring_cluster_admin', pg_catalog.current_database() = 'postgres', pg_catalog.inet_client_addr() = '127.0.0.1'::INET, pg_catalog.inet_server_addr() = '127.0.0.1'::INET, pg_catalog.inet_server_port() = 5432, NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE), control.system_identifier::TEXT = $1 FROM pg_catalog.pg_control_system() AS control",
    )
    .bind(system_identifier)
    .fetch_one(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
    let version: i32 = row
        .try_get(0)
        .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
    let checks = (1..=7)
        .map(|index| {
            row.try_get::<bool, _>(index)
                .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !(160000..170000).contains(&version) || checks.iter().any(|check| !check) {
        return Err(ProvisionerErrorV1::FinalConnectionContract);
    }
    Ok(())
}

pub(crate) async fn verify_application_connection(
    connection: &mut PgConnection,
    role: &str,
) -> Result<(), ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT current_user = session_user AND current_user = $1, pg_catalog.current_database() = 'starring_runtime_staging', pg_catalog.inet_client_addr() = '127.0.0.1'::INET, pg_catalog.inet_server_addr() = '127.0.0.1'::INET, pg_catalog.inet_server_port() = 5432, NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE)",
    )
    .bind(role)
    .fetch_one(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
    for index in 0..6 {
        let exact: bool = row
            .try_get(index)
            .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
        if !exact {
            return Err(ProvisionerErrorV1::FinalConnectionContract);
        }
    }
    Ok(())
}

pub(crate) async fn verify_admin_target_connection(
    connection: &mut PgConnection,
) -> Result<(), ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT current_user = session_user AND current_user = 'starring_cluster_admin' AND pg_catalog.current_database() = 'starring_runtime_staging' AND pg_catalog.inet_client_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_addr() = '127.0.0.1'::INET AND pg_catalog.inet_server_port() = 5432 AND NOT COALESCE((SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()), TRUE) AND (SELECT owner.rolname = 'starring_owner' FROM pg_catalog.pg_database AS database_row INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = database_row.datdba WHERE database_row.datname = 'starring_runtime_staging') AND (SELECT owner.rolname = 'starring_owner' FROM pg_catalog.pg_namespace AS namespace INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = namespace.nspowner WHERE namespace.nspname = 'public')",
    )
    .fetch_one(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalConnectionContract)?;
    if !exact {
        return Err(ProvisionerErrorV1::FinalConnectionContract);
    }
    Ok(())
}

async fn verify_final_roles(connection: &mut PgConnection) -> Result<(), ProvisionerErrorV1> {
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    let rows = sqlx::query(
        "SELECT rolname, rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolinherit AND NOT rolreplication AND NOT rolbypassrls AND rolconnlimit = 4 AND rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND rolpassword LIKE 'SCRAM-SHA-256$4096:%' FROM pg_catalog.pg_authid WHERE rolname = ANY($1::TEXT[]) ORDER BY rolname",
    )
    .bind(&roles)
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalRoleContract)?;
    if rows.len() != APPLICATION_DATABASE_IDENTITIES.len()
        || rows
            .iter()
            .any(|row| row.try_get::<bool, _>(1).map_or(true, |enabled| !enabled))
    {
        return Err(ProvisionerErrorV1::FinalRoleContract);
    }
    let isolated: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[])) OR membership.member IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[])))",
    )
    .bind(&roles)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalRoleContract)?;
    let settings_absent: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting INNER JOIN pg_catalog.pg_roles AS role ON role.oid = setting.setrole WHERE role.rolname = ANY($1::TEXT[]))",
    )
    .bind(&roles)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalRoleContract)?;
    let fixed_roles: bool = sqlx::query_scalar(
        "SELECT (SELECT NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolinherit AND NOT rolreplication AND NOT rolbypassrls AND rolconnlimit = 0 AND rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND rolpassword IS NULL AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole = role.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid = role.oid OR membership.member = role.oid) FROM pg_catalog.pg_authid AS role WHERE rolname = 'starring_owner') AND (SELECT rolcanlogin AND rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolinherit AND NOT rolreplication AND NOT rolbypassrls AND rolconnlimit = 2 AND rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND rolpassword LIKE 'SCRAM-SHA-256$4096:%' AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole = role.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid = role.oid OR membership.member = role.oid) FROM pg_catalog.pg_authid AS role WHERE rolname = 'starring_cluster_admin') AND (SELECT owner.rolname = 'starring_owner' FROM pg_catalog.pg_database AS database_row INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = database_row.datdba WHERE database_row.datname = 'starring_runtime_staging')",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalRoleContract)?;
    if !isolated || !settings_absent || !fixed_roles {
        return Err(ProvisionerErrorV1::FinalRoleContract);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExpectedHbaRuleV1 {
    line_number: i32,
    connection_type: &'static str,
    databases: Vec<&'static str>,
    users: Vec<&'static str>,
    address: Option<&'static str>,
    netmask: Option<&'static str>,
    auth_method: &'static str,
    options: Vec<&'static str>,
}

fn expected_hba_rules() -> Vec<ExpectedHbaRuleV1> {
    let runtime = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .filter(|identity| identity.service == RUNTIME_KEYCHAIN_SERVICE)
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    let api = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .filter(|identity| identity.service != RUNTIME_KEYCHAIN_SERVICE)
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    vec![
        expected_rule(
            1,
            "hostnossl",
            vec![DATABASE_NAME],
            runtime.clone(),
            Some(DATABASE_HOST),
            Some("255.255.255.255"),
            "scram-sha-256",
        ),
        expected_rule(
            2,
            "host",
            vec!["all"],
            runtime.clone(),
            Some("0.0.0.0"),
            Some("0.0.0.0"),
            "reject",
        ),
        expected_rule(
            3,
            "host",
            vec!["all"],
            runtime.clone(),
            Some("::"),
            Some("::"),
            "reject",
        ),
        expected_rule(4, "local", vec!["all"], runtime, None, None, "reject"),
        expected_rule(
            5,
            "hostnossl",
            vec![DATABASE_NAME],
            api.clone(),
            Some(DATABASE_HOST),
            Some("255.255.255.255"),
            "scram-sha-256",
        ),
        expected_rule(
            6,
            "host",
            vec!["all"],
            api.clone(),
            Some("0.0.0.0"),
            Some("0.0.0.0"),
            "reject",
        ),
        expected_rule(
            7,
            "host",
            vec!["all"],
            api.clone(),
            Some("::"),
            Some("::"),
            "reject",
        ),
        expected_rule(8, "local", vec!["all"], api, None, None, "reject"),
        expected_rule(
            9,
            "hostnossl",
            vec!["postgres", DATABASE_NAME],
            vec![CLUSTER_ADMIN_ROLE],
            Some(DATABASE_HOST),
            Some("255.255.255.255"),
            "scram-sha-256",
        ),
        expected_rule(
            10,
            "host",
            vec!["all"],
            vec!["all"],
            Some("0.0.0.0"),
            Some("0.0.0.0"),
            "reject",
        ),
        expected_rule(
            11,
            "host",
            vec!["all"],
            vec!["all"],
            Some("::"),
            Some("::"),
            "reject",
        ),
        expected_rule(12, "local", vec!["all"], vec!["all"], None, None, "reject"),
        expected_rule(
            13,
            "host",
            vec!["replication"],
            vec!["all"],
            Some("0.0.0.0"),
            Some("0.0.0.0"),
            "reject",
        ),
        expected_rule(
            14,
            "host",
            vec!["replication"],
            vec!["all"],
            Some("::"),
            Some("::"),
            "reject",
        ),
        expected_rule(
            15,
            "local",
            vec!["replication"],
            vec!["all"],
            None,
            None,
            "reject",
        ),
    ]
}

fn expected_rule(
    line_number: i32,
    connection_type: &'static str,
    databases: Vec<&'static str>,
    users: Vec<&'static str>,
    address: Option<&'static str>,
    netmask: Option<&'static str>,
    auth_method: &'static str,
) -> ExpectedHbaRuleV1 {
    ExpectedHbaRuleV1 {
        line_number,
        connection_type,
        databases,
        users,
        address,
        netmask,
        auth_method,
        options: Vec::new(),
    }
}

pub(crate) async fn verify_final_hba(
    connection: &mut PgConnection,
) -> Result<(), ProvisionerErrorV1> {
    let rows = sqlx::query(
        "SELECT line_number, type, database, user_name, address, netmask, auth_method, COALESCE(options, ARRAY[]::TEXT[]), error FROM pg_catalog.pg_hba_file_rules ORDER BY line_number",
    )
    .fetch_all(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
    let expected = expected_hba_rules();
    if rows.len() != expected.len() {
        return Err(ProvisionerErrorV1::FinalHbaContract);
    }
    for (row, expected) in rows.iter().zip(expected) {
        let line_number: i32 = row
            .try_get(0)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let connection_type: String = row
            .try_get(1)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let databases: Vec<String> = row
            .try_get(2)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let users: Vec<String> = row
            .try_get(3)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let address: Option<String> = row
            .try_get(4)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let netmask: Option<String> = row
            .try_get(5)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let auth_method: String = row
            .try_get(6)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let options: Vec<String> = row
            .try_get(7)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        let error: Option<String> = row
            .try_get(8)
            .map_err(|_| ProvisionerErrorV1::FinalHbaContract)?;
        if line_number != expected.line_number
            || connection_type != expected.connection_type
            || databases
                != expected
                    .databases
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            || users
                != expected
                    .users
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            || address.as_deref() != expected.address
            || netmask.as_deref() != expected.netmask
            || auth_method != expected.auth_method
            || options
                != expected
                    .options
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            || error.is_some()
        {
            return Err(ProvisionerErrorV1::FinalHbaContract);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_hba_manifest_is_exactly_fifteen_ordered_rules() {
        let rules = expected_hba_rules();
        assert_eq!(rules.len(), 15);
        assert!(rules
            .iter()
            .enumerate()
            .all(|(index, rule)| rule.line_number == index as i32 + 1));
        assert_eq!(rules[0].users.len(), 5);
        assert_eq!(rules[4].users.len(), 15);
        assert_eq!(rules[8].users, [CLUSTER_ADMIN_ROLE]);
        assert_eq!(rules[12].databases, ["replication"]);
        assert_eq!(rules[14].connection_type, "local");
    }

    #[test]
    fn final_contract_constants_are_fixed() {
        assert_eq!(DATABASE_HOST, "127.0.0.1");
        assert_eq!(crate::identity::DATABASE_PORT, 5432);
        assert_eq!(DATABASE_NAME, "starring_runtime_staging");
        assert_eq!(crate::identity::OWNER_ROLE, "starring_owner");
        let source = include_str!("final_verify.rs");
        assert!(source.contains("namespace.nspname = 'public'"));
        assert!(source.contains("owner.rolname = 'starring_owner'"));
        assert!(source.contains("rolconnlimit = 2"));
        assert!(source.contains("pg_catalog.pg_db_role_setting"));
        assert!(source.contains("pg_catalog.pg_auth_members"));
    }
}
