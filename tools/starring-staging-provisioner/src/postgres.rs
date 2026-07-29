use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};
use zeroize::Zeroizing;

use crate::crypto::{valid_scram_verifier, GeneratedSecretsV1};
use crate::identity::{
    peer_connect_options, APPLICATION_DATABASE_IDENTITIES, CLUSTER_ADMIN_ROLE, DATABASE_NAME,
    PEER_SYSTEM_USER,
};
use crate::{ProvisionerErrorV1, StagingAcknowledgementV1};

const VERIFY_CLUSTER_SQL: &str = r#"
SELECT
    pg_catalog.current_setting('server_version_num')::INTEGER,
    pg_catalog.current_database() = 'starring_runtime_staging',
    current_user = session_user
        AND current_user = 'starring_cluster_admin',
    pg_catalog.inet_client_addr() IS NULL,
    control.system_identifier::TEXT = $1,
    admin.rolsuper
        AND admin.rolcanlogin
        AND NOT admin.rolcreatedb
        AND NOT admin.rolcreaterole
        AND NOT admin.rolinherit
        AND NOT admin.rolreplication
        AND NOT admin.rolbypassrls
        AND admin.rolconnlimit = 2
        AND admin.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
        AND admin.rolpassword IS NULL,
    owner.rolname = 'starring_owner'
        AND NOT owner.rolcanlogin
        AND NOT owner.rolsuper
        AND NOT owner.rolcreatedb
        AND NOT owner.rolcreaterole
        AND NOT owner.rolinherit
        AND NOT owner.rolreplication
        AND NOT owner.rolbypassrls
        AND owner.rolconnlimit = 0
        AND owner.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
        AND owner.rolpassword IS NULL,
    database_owner.rolname = 'starring_owner',
    pg_catalog.current_setting('data_checksums') = 'on',
    schema_owner.rolname = 'starring_owner'
FROM pg_catalog.pg_control_system() AS control
INNER JOIN pg_catalog.pg_authid AS admin
    ON admin.rolname = 'starring_cluster_admin'
INNER JOIN pg_catalog.pg_authid AS owner
    ON owner.rolname = 'starring_owner'
INNER JOIN pg_catalog.pg_database AS database_row
    ON database_row.datname = 'starring_runtime_staging'
INNER JOIN pg_catalog.pg_roles AS database_owner
    ON database_owner.oid = database_row.datdba
INNER JOIN pg_catalog.pg_namespace AS public_schema
    ON public_schema.nspname = 'public'
INNER JOIN pg_catalog.pg_roles AS schema_owner
    ON schema_owner.oid = public_schema.nspowner
WHERE NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_db_role_setting AS setting
    WHERE setting.setrole = admin.oid
)
AND NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_auth_members AS membership
    WHERE membership.roleid = admin.oid
        OR membership.member = admin.oid
)
AND NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_db_role_setting AS setting
    WHERE setting.setrole = owner.oid
)
AND NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_auth_members AS membership
    WHERE membership.roleid = owner.oid
        OR membership.member = owner.oid
)
"#;

const VERIFY_QUIESCENCE_SQL: &str = r#"
SELECT
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND activity.backend_type = 'client backend'
    ),
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts
    )
"#;

const VERIFY_OWNER_MEMBERSHIP_SQL: &str = r#"
SELECT NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_auth_members AS membership
    WHERE membership.roleid = pg_catalog.to_regrole('starring_owner')
        OR membership.member = pg_catalog.to_regrole('starring_owner')
)
"#;

pub struct StagingPostgresSessionV1 {
    connection: PgConnection,
    system_identifier: String,
}

impl StagingPostgresSessionV1 {
    pub async fn connect_and_preflight(
        acknowledgement: &StagingAcknowledgementV1,
    ) -> Result<Self, ProvisionerErrorV1> {
        let mut connection = PgConnection::connect_with(&peer_connect_options())
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseConnection)?;
        verify_cluster(&mut connection, acknowledgement.system_identifier()).await?;
        verify_bootstrap_authentication_manifest(&mut connection).await?;
        verify_quiescence(&mut connection).await?;
        verify_quarantine(&mut connection).await?;
        Ok(Self {
            connection,
            system_identifier: acknowledgement.system_identifier().to_owned(),
        })
    }

    pub async fn apply_verifiers(
        mut self,
        secrets: &GeneratedSecretsV1,
    ) -> Result<(), ProvisionerErrorV1> {
        let mut transaction = self
            .connection
            .begin()
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *transaction)
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
        verify_cluster(&mut transaction, &self.system_identifier).await?;
        verify_quiescence(&mut transaction).await?;
        verify_quarantine(&mut transaction).await?;
        for secret in secrets.database() {
            let sql = alter_role_password_sql(secret.identity().role, secret.verifier())?;
            sqlx::query(&sql)
                .execute(&mut *transaction)
                .await
                .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
        }
        let admin_sql = alter_role_password_sql(CLUSTER_ADMIN_ROLE, secrets.admin().verifier())?;
        sqlx::query(&admin_sql)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseMutation)?;
        verify_postmutation(&mut transaction, secrets).await?;
        transaction
            .commit()
            .await
            .map_err(|_| ProvisionerErrorV1::DatabaseCommitIndeterminate)
    }
}

async fn verify_cluster(
    connection: &mut PgConnection,
    system_identifier: &str,
) -> Result<(), ProvisionerErrorV1> {
    let row = sqlx::query(VERIFY_CLUSTER_SQL)
        .bind(system_identifier)
        .fetch_optional(&mut *connection)
        .await
        .map_err(|_| ProvisionerErrorV1::ClusterContract)?
        .ok_or(ProvisionerErrorV1::ClusterContract)?;
    let version: i32 = row
        .try_get(0)
        .map_err(|_| ProvisionerErrorV1::ClusterContract)?;
    let checks = (1..=9)
        .map(|index| {
            row.try_get::<bool, _>(index)
                .map_err(|_| ProvisionerErrorV1::ClusterContract)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !(160000..170000).contains(&version) || checks.iter().any(|check| !check) {
        return Err(ProvisionerErrorV1::ClusterContract);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BootstrapHbaRuleV1 {
    line_number: i32,
    connection_type: &'static str,
    databases: Vec<&'static str>,
    users: Vec<&'static str>,
    address: Option<&'static str>,
    netmask: Option<&'static str>,
    auth_method: &'static str,
    options: Vec<&'static str>,
}

fn bootstrap_hba_rules() -> Vec<BootstrapHbaRuleV1> {
    vec![
        BootstrapHbaRuleV1 {
            line_number: 1,
            connection_type: "local",
            databases: vec!["postgres", DATABASE_NAME],
            users: vec![CLUSTER_ADMIN_ROLE],
            address: None,
            netmask: None,
            auth_method: "peer",
            options: vec!["map=starring_bootstrap"],
        },
        bootstrap_reject_rule(2, "host", vec!["all"], Some("0.0.0.0"), Some("0.0.0.0")),
        bootstrap_reject_rule(3, "host", vec!["all"], Some("::"), Some("::")),
        bootstrap_reject_rule(4, "local", vec!["all"], None, None),
        bootstrap_reject_rule(
            5,
            "host",
            vec!["replication"],
            Some("0.0.0.0"),
            Some("0.0.0.0"),
        ),
        bootstrap_reject_rule(6, "host", vec!["replication"], Some("::"), Some("::")),
        bootstrap_reject_rule(7, "local", vec!["replication"], None, None),
    ]
}

fn bootstrap_reject_rule(
    line_number: i32,
    connection_type: &'static str,
    databases: Vec<&'static str>,
    address: Option<&'static str>,
    netmask: Option<&'static str>,
) -> BootstrapHbaRuleV1 {
    BootstrapHbaRuleV1 {
        line_number,
        connection_type,
        databases,
        users: vec!["all"],
        address,
        netmask,
        auth_method: "reject",
        options: Vec::new(),
    }
}

async fn verify_bootstrap_authentication_manifest(
    connection: &mut PgConnection,
) -> Result<(), ProvisionerErrorV1> {
    let rows = sqlx::query(
        "SELECT line_number, type, database, user_name, address, netmask, auth_method, COALESCE(options, ARRAY[]::TEXT[]), error FROM pg_catalog.pg_hba_file_rules ORDER BY line_number",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    let expected = bootstrap_hba_rules();
    if rows.len() != expected.len() {
        return Err(ProvisionerErrorV1::PeerContract);
    }
    for (row, expected) in rows.iter().zip(expected) {
        let line_number: i32 = row
            .try_get(0)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let connection_type: String = row
            .try_get(1)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let databases: Vec<String> = row
            .try_get(2)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let users: Vec<String> = row
            .try_get(3)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let address: Option<String> = row
            .try_get(4)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let netmask: Option<String> = row
            .try_get(5)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let auth_method: String = row
            .try_get(6)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let options: Vec<String> = row
            .try_get(7)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
        let error: Option<String> = row
            .try_get(8)
            .map_err(|_| ProvisionerErrorV1::PeerContract)?;
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
            return Err(ProvisionerErrorV1::PeerContract);
        }
    }
    let ident_rows = sqlx::query(
        "SELECT line_number, map_name, sys_name, pg_username, error FROM pg_catalog.pg_ident_file_mappings ORDER BY line_number",
    )
    .fetch_all(connection)
    .await
    .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    if ident_rows.len() != 1 {
        return Err(ProvisionerErrorV1::PeerContract);
    }
    let ident = &ident_rows[0];
    let line_number: i32 = ident
        .try_get(0)
        .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    let map_name: String = ident
        .try_get(1)
        .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    let system_user: String = ident
        .try_get(2)
        .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    let database_user: String = ident
        .try_get(3)
        .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    let error: Option<String> = ident
        .try_get(4)
        .map_err(|_| ProvisionerErrorV1::PeerContract)?;
    if line_number != 1
        || map_name != "starring_bootstrap"
        || system_user != PEER_SYSTEM_USER
        || database_user != CLUSTER_ADMIN_ROLE
        || error.is_some()
    {
        return Err(ProvisionerErrorV1::PeerContract);
    }
    Ok(())
}

async fn verify_quiescence(connection: &mut PgConnection) -> Result<(), ProvisionerErrorV1> {
    let row = sqlx::query(VERIFY_QUIESCENCE_SQL)
        .fetch_one(connection)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseQuiescence)?;
    let no_clients: bool = row
        .try_get(0)
        .map_err(|_| ProvisionerErrorV1::DatabaseQuiescence)?;
    let no_prepared: bool = row
        .try_get(1)
        .map_err(|_| ProvisionerErrorV1::DatabaseQuiescence)?;
    if !no_clients || !no_prepared {
        return Err(ProvisionerErrorV1::DatabaseQuiescence);
    }
    Ok(())
}

async fn verify_quarantine(connection: &mut PgConnection) -> Result<(), ProvisionerErrorV1> {
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| identity.role)
        .collect::<Vec<_>>();
    let rows = sqlx::query(
        "SELECT rolname, NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolinherit AND NOT rolreplication AND NOT rolbypassrls AND rolconnlimit = 4 AND rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname = ANY($1::TEXT[]) ORDER BY rolname",
    )
    .bind(&roles)
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::RoleQuarantine)?;
    if rows.len() != APPLICATION_DATABASE_IDENTITIES.len()
        || rows.iter().any(|row| {
            row.try_get::<bool, _>(1)
                .map_or(true, |quarantined| !quarantined)
        })
    {
        return Err(ProvisionerErrorV1::RoleQuarantine);
    }
    let isolated: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[])) OR membership.member IN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = ANY($1::TEXT[])))",
    )
    .bind(&roles)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::RoleQuarantine)?;
    let settings_absent: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting INNER JOIN pg_catalog.pg_roles AS role ON role.oid = setting.setrole WHERE role.rolname = ANY($1::TEXT[]))",
    )
    .bind(&roles)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::RoleQuarantine)?;
    let owner_isolated: bool = sqlx::query_scalar(VERIFY_OWNER_MEMBERSHIP_SQL)
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ProvisionerErrorV1::RoleQuarantine)?;
    if !isolated || !settings_absent || !owner_isolated {
        return Err(ProvisionerErrorV1::RoleQuarantine);
    }
    Ok(())
}

async fn verify_postmutation(
    connection: &mut PgConnection,
    secrets: &GeneratedSecretsV1,
) -> Result<(), ProvisionerErrorV1> {
    for secret in secrets.database() {
        let exact: bool = sqlx::query_scalar(
            "SELECT NOT rolcanlogin AND rolpassword = $2 FROM pg_catalog.pg_authid WHERE rolname = $1",
        )
        .bind(secret.identity().role)
        .bind(secret.verifier())
        .fetch_optional(&mut *connection)
        .await
        .map_err(|_| ProvisionerErrorV1::DatabaseVerification)?
        .ok_or(ProvisionerErrorV1::DatabaseVerification)?;
        if !exact {
            return Err(ProvisionerErrorV1::DatabaseVerification);
        }
    }
    let admin_exact: bool = sqlx::query_scalar(
        "SELECT rolcanlogin AND rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolinherit AND NOT rolreplication AND NOT rolbypassrls AND rolconnlimit = 2 AND rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE AND rolpassword = $1 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting WHERE setting.setrole = role.oid) AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership WHERE membership.roleid = role.oid OR membership.member = role.oid) FROM pg_catalog.pg_authid AS role WHERE rolname = 'starring_cluster_admin'",
    )
    .bind(secrets.admin().verifier())
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::DatabaseVerification)?
    .ok_or(ProvisionerErrorV1::DatabaseVerification)?;
    let owner_exact: bool = sqlx::query_scalar(
        "SELECT NOT rolcanlogin AND rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname = 'starring_owner'",
    )
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| ProvisionerErrorV1::DatabaseVerification)?
    .ok_or(ProvisionerErrorV1::DatabaseVerification)?;
    if !admin_exact || !owner_exact {
        return Err(ProvisionerErrorV1::DatabaseVerification);
    }
    Ok(())
}

pub fn alter_role_password_sql(
    role: &str,
    verifier: &str,
) -> Result<Zeroizing<String>, ProvisionerErrorV1> {
    if !valid_scram_verifier(verifier) {
        return Err(ProvisionerErrorV1::Scram);
    }
    let role = match role {
        "starring_identity_oauth" => "starring_identity_oauth",
        "starring_identity_issuer" => "starring_identity_issuer",
        "starring_identity_session" => "starring_identity_session",
        "starring_identity_security" => "starring_identity_security",
        "starring_installation_authority_reader" => "starring_installation_authority_reader",
        "starring_authorized_snapshot_reader" => "starring_authorized_snapshot_reader",
        "starring_promotion_executor" => "starring_promotion_executor",
        "starring_decision_reader" => "starring_decision_reader",
        "starring_decision_approval" => "starring_decision_approval",
        "starring_decision_rejection" => "starring_decision_rejection",
        "starring_decision_apply" => "starring_decision_apply",
        "starring_decision_cancellation" => "starring_decision_cancellation",
        "starring_deployment_status_reader" => "starring_deployment_status_reader",
        "starring_operational_deployment_status_reader" => {
            "starring_operational_deployment_status_reader"
        }
        "starring_runtime_execution" => "starring_runtime_execution",
        "starring_runtime_exact_target" => "starring_runtime_exact_target",
        "starring_runtime_panel" => "starring_runtime_panel",
        "starring_runtime_serving" => "starring_runtime_serving",
        "starring_runtime_interaction" => "starring_runtime_interaction",
        "starring_cluster_admin" => "starring_cluster_admin",
        _ => return Err(ProvisionerErrorV1::IdentityManifest),
    };
    Ok(Zeroizing::new(format!(
        "ALTER ROLE {role} PASSWORD '{verifier}'"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::scram_verifier;

    #[test]
    fn role_mutation_is_fixed_whitelist_and_accepts_only_scram_verifier() {
        let verifier = scram_verifier(b"pencil", b"0123456789abcdef").unwrap();
        for identity in APPLICATION_DATABASE_IDENTITIES {
            let sql = alter_role_password_sql(identity.role, &verifier).unwrap();
            assert!(sql.starts_with(&format!("ALTER ROLE {} PASSWORD '", identity.role)));
            assert!(sql.ends_with('\''));
            assert!(!sql.contains("pencil"));
        }
        assert!(alter_role_password_sql(CLUSTER_ADMIN_ROLE, &verifier).is_ok());
        assert!(alter_role_password_sql(crate::identity::OWNER_ROLE, &verifier).is_err());
        assert!(alter_role_password_sql("starring_unlisted", &verifier).is_err());
        assert!(alter_role_password_sql(CLUSTER_ADMIN_ROLE, "plaintext").is_err());
    }

    #[test]
    fn constants_bind_the_exact_staging_target() {
        assert_eq!(crate::identity::DATABASE_NAME, "starring_runtime_staging");
        assert_eq!(crate::identity::OWNER_ROLE, "starring_owner");
        assert_eq!(CLUSTER_ADMIN_ROLE, "starring_cluster_admin");
        for required in [
            "NOT admin.rolcreatedb",
            "NOT admin.rolcreaterole",
            "NOT admin.rolinherit",
            "NOT admin.rolreplication",
            "NOT admin.rolbypassrls",
            "admin.rolconnlimit = 2",
            "admin.rolvaliduntil = 'infinity'",
            "admin.rolpassword IS NULL",
            "schema_owner.rolname = 'starring_owner'",
            "pg_catalog.pg_db_role_setting",
            "pg_catalog.pg_auth_members",
        ] {
            assert!(VERIFY_CLUSTER_SQL.contains(required), "{required}");
        }
        let hba = bootstrap_hba_rules();
        assert_eq!(hba.len(), 7);
        assert!(hba
            .iter()
            .enumerate()
            .all(|(index, rule)| rule.line_number == index as i32 + 1));
        assert_eq!(hba[0].options, ["map=starring_bootstrap"]);
        assert_eq!(PEER_SYSTEM_USER, "jungbogeon");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore]
    async fn temporary_postgres_accepts_only_generated_scram_verifiers() {
        let system_identifier = std::env::var("STARRING_TEST_SYSTEM_IDENTIFIER").unwrap();
        let acknowledgement = crate::StagingAcknowledgementV1::parse(
            &system_identifier,
            &format!(
                "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
            ),
        )
        .unwrap();
        let session = StagingPostgresSessionV1::connect_and_preflight(&acknowledgement)
            .await
            .unwrap();
        let secrets = GeneratedSecretsV1::generate().unwrap();
        session.apply_verifiers(&secrets).await.unwrap();
        let mut verification = PgConnection::connect_with(&peer_connect_options())
            .await
            .unwrap();
        for secret in secrets.database() {
            let exact: bool = sqlx::query_scalar(
                "SELECT NOT rolcanlogin AND rolpassword = $2 FROM pg_catalog.pg_authid WHERE rolname = $1",
            )
            .bind(secret.identity().role)
            .bind(secret.verifier())
            .fetch_one(&mut verification)
            .await
            .unwrap();
            assert!(exact);
        }
        let admin_exact: bool = sqlx::query_scalar(
            "SELECT rolcanlogin AND rolsuper AND rolpassword = $1 FROM pg_catalog.pg_authid WHERE rolname = 'starring_cluster_admin'",
        )
        .bind(secrets.admin().verifier())
        .fetch_one(&mut verification)
        .await
        .unwrap();
        let owner_exact: bool = sqlx::query_scalar(
            "SELECT NOT rolcanlogin AND rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname = 'starring_owner'",
        )
        .fetch_one(&mut verification)
        .await
        .unwrap();
        assert!(admin_exact);
        assert!(owner_exact);
    }
}
