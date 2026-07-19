use super::PostgresInstallationAuthoritySource;
use crate::ProductDatabaseFailureV1;

const FUNCTION_IDENTITY: &str =
    "public.starring_product_installation_authority_read_v1(text,text,bytea)";
const FUNCTION_RESULT: &str = "TABLE(principal_id text, acting_user_id text, principal_disabled boolean, session_digest bytea, session_principal_id text, oauth_state_digest_length integer, last_seen_at timestamp with time zone, idle_expires_at timestamp with time zone, absolute_expires_at timestamp with time zone, revoked_at timestamp with time zone, installation_tenant_id text, installation_id text, tenant_id text, tenant_lifecycle_state text, installation_lifecycle_state text, discord_application_id text, discord_guild_id text, current_authority_revision bigint, authority_tenant_id text, authority_installation_id text, authority_revision bigint, authority_payload_digest text, database_now timestamp with time zone)";
const PROBE_IDENTITY: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstallationAuthorityReadinessErrorV1 {
    #[error("installation authority database contract is invalid")]
    ContractMismatch,
    #[error("installation authority database capability is missing")]
    CapabilityMissing,
    #[error("installation authority database capability is excessive")]
    ExcessCapability,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

#[derive(sqlx::FromRow)]
struct InstallationAuthorityReadinessRow {
    function_contract_valid: bool,
    owner_contract_valid: bool,
    public_execute_revoked: bool,
    caller_session_direct: bool,
    caller_execute: bool,
    caller_execute_grantable: bool,
    unexpected_function_grant: bool,
    caller_database_connect: bool,
    caller_database_create: bool,
    caller_database_temporary: bool,
    caller_schema_usage: bool,
    caller_schema_create: bool,
    caller_has_table_privilege: bool,
    caller_has_column_privilege: bool,
    caller_has_role_membership: bool,
    caller_role_excessive: bool,
    caller_is_owner_member: bool,
}

impl InstallationAuthorityReadinessRow {
    fn verify(self) -> Result<(), InstallationAuthorityReadinessErrorV1> {
        if !self.function_contract_valid
            || !self.owner_contract_valid
            || !self.public_execute_revoked
            || !self.caller_session_direct
        {
            return Err(InstallationAuthorityReadinessErrorV1::ContractMismatch);
        }
        if !self.caller_execute || !self.caller_database_connect || !self.caller_schema_usage {
            return Err(InstallationAuthorityReadinessErrorV1::CapabilityMissing);
        }
        if self.caller_execute_grantable
            || self.caller_database_create
            || self.caller_database_temporary
            || self.unexpected_function_grant
            || self.caller_schema_create
            || self.caller_has_table_privilege
            || self.caller_has_column_privilege
            || self.caller_has_role_membership
            || self.caller_role_excessive
            || self.caller_is_owner_member
        {
            return Err(InstallationAuthorityReadinessErrorV1::ExcessCapability);
        }
        Ok(())
    }
}

impl PostgresInstallationAuthoritySource {
    pub async fn verify_readiness(&self) -> Result<(), InstallationAuthorityReadinessErrorV1> {
        let mut transaction = self.pool.begin().await.map_err(readiness_database)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        let timeout = self.config.statement_timeout();
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
             pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
        )
        .bind(timeout)
        .execute(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        let contract = sqlx::query_as::<_, InstallationAuthorityReadinessRow>(
            "WITH target AS ( \
               SELECT pg_catalog.to_regprocedure($1) AS function_oid, \
                pg_catalog.to_regnamespace('public') AS schema_oid \
             ), expected_relations(relation_oid) AS ( \
               VALUES \
                (pg_catalog.to_regclass('public.product_principals')), \
                (pg_catalog.to_regclass('public.product_auth_sessions')), \
                (pg_catalog.to_regclass('public.product_tenants')), \
                (pg_catalog.to_regclass('public.automation_installations')), \
                (pg_catalog.to_regclass( \
                 'public.automation_installation_authority_versions')) \
             ), function_contract AS ( \
               SELECT function_row.oid, function_row.proowner, function_row.proacl, \
                function_row.prosecdef, function_row.proisstrict, \
                function_row.provolatile, function_row.proparallel, \
                function_row.proretset, function_row.prorows, function_row.proconfig, \
                language.lanname, pg_catalog.pg_get_function_result(function_row.oid) \
                 AS function_result \
               FROM target \
               LEFT JOIN pg_catalog.pg_proc AS function_row \
                ON function_row.oid = target.function_oid \
               LEFT JOIN pg_catalog.pg_language AS language \
                ON language.oid = function_row.prolang \
             ), relation_contract AS ( \
               SELECT pg_catalog.count(relation.oid) = 5 \
                 AND pg_catalog.count(DISTINCT relation.relowner) = 1 \
                 AND pg_catalog.bool_and(relation.relkind = 'r') \
                 AND pg_catalog.bool_and(relation.relowner = function_contract.proowner) \
                  AS valid \
               FROM function_contract \
               CROSS JOIN expected_relations \
               LEFT JOIN pg_catalog.pg_class AS relation \
                ON relation.oid = expected_relations.relation_oid \
             ), caller_role AS ( \
               SELECT role.oid, role.rolsuper, role.rolcreatedb, role.rolcreaterole, \
                role.rolcanlogin, role.rolreplication, role.rolbypassrls \
               FROM pg_catalog.pg_roles AS role \
               WHERE role.rolname = current_user \
             ), owner_role AS ( \
               SELECT role.oid, role.rolname, role.rolsuper, role.rolcreatedb, \
                role.rolcreaterole, role.rolcanlogin, role.rolreplication, \
                role.rolbypassrls \
               FROM function_contract \
               INNER JOIN pg_catalog.pg_roles AS role \
                ON role.oid = function_contract.proowner \
             ) \
             SELECT COALESCE( \
               function_contract.oid IS NOT NULL \
                AND function_contract.prosecdef \
                AND function_contract.proisstrict \
                AND function_contract.provolatile = 'v' \
                AND function_contract.proparallel = 'u' \
                AND function_contract.proretset \
                AND function_contract.prorows = 1 \
                AND function_contract.proconfig = ARRAY['search_path=pg_catalog']::TEXT[] \
                AND function_contract.lanname = 'sql' \
                AND function_contract.function_result = $2, FALSE \
              ) AS function_contract_valid, \
              COALESCE( \
               relation_contract.valid \
                AND NOT owner_role.rolsuper \
                AND NOT owner_role.rolcreatedb \
                AND NOT owner_role.rolcreaterole \
                AND NOT owner_role.rolcanlogin \
                AND NOT owner_role.rolreplication \
                AND NOT owner_role.rolbypassrls \
                AND NOT EXISTS ( \
                 SELECT 1 \
                 FROM pg_catalog.pg_auth_members AS owner_membership \
                 WHERE owner_membership.member = owner_role.oid \
                  OR owner_membership.roleid = owner_role.oid \
                ), FALSE \
              ) AS owner_contract_valid, \
              COALESCE(NOT EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_contract.proacl, \
                pg_catalog.acldefault('f', function_contract.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE' \
              ), FALSE) AS public_execute_revoked, \
              COALESCE(current_user = session_user \
               AND caller_role.rolcanlogin, FALSE) AS caller_session_direct, \
              COALESCE(EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_contract.proacl, \
                pg_catalog.acldefault('f', function_contract.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = caller_role.oid \
                AND privilege.privilege_type = 'EXECUTE' \
              ), FALSE) AS caller_execute, \
              COALESCE(EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_contract.proacl, \
                pg_catalog.acldefault('f', function_contract.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = caller_role.oid \
                AND privilege.privilege_type = 'EXECUTE' \
                AND privilege.is_grantable \
              ), FALSE) AS caller_execute_grantable, \
              COALESCE(EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_contract.proacl, \
                pg_catalog.acldefault('f', function_contract.proowner) \
               )) AS privilege \
               WHERE privilege.grantee <> 0 \
                AND privilege.grantee <> function_contract.proowner \
                AND privilege.grantee <> caller_role.oid \
                AND privilege.privilege_type = 'EXECUTE' \
              ), TRUE) AS unexpected_function_grant, \
              COALESCE(pg_catalog.has_database_privilege( \
               current_user, pg_catalog.current_database(), 'CONNECT'), FALSE) \
                AS caller_database_connect, \
              COALESCE(pg_catalog.has_database_privilege( \
               current_user, pg_catalog.current_database(), 'CREATE'), FALSE) \
                AS caller_database_create, \
              COALESCE(pg_catalog.has_database_privilege( \
               current_user, pg_catalog.current_database(), 'TEMPORARY'), FALSE) \
                AS caller_database_temporary, \
              COALESCE(pg_catalog.has_schema_privilege( \
               current_user, target.schema_oid, 'USAGE'), FALSE) \
                AS caller_schema_usage, \
              COALESCE(pg_catalog.has_schema_privilege( \
               current_user, target.schema_oid, 'CREATE'), FALSE) \
                AS caller_schema_create, \
              EXISTS ( \
               SELECT 1 \
               FROM expected_relations \
               INNER JOIN pg_catalog.pg_class AS relation \
                ON relation.oid = expected_relations.relation_oid \
               CROSS JOIN (VALUES \
                ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'), \
                ('TRUNCATE'), ('REFERENCES'), ('TRIGGER') \
               ) AS checked_privilege(name) \
               WHERE pg_catalog.has_table_privilege( \
                current_user, relation.oid, checked_privilege.name) \
              ) AS caller_has_table_privilege, \
              EXISTS ( \
               SELECT 1 \
               FROM expected_relations \
               INNER JOIN pg_catalog.pg_class AS relation \
                ON relation.oid = expected_relations.relation_oid \
               CROSS JOIN (VALUES \
                ('SELECT'), ('INSERT'), ('UPDATE'), ('REFERENCES') \
               ) AS checked_privilege(name) \
               WHERE pg_catalog.has_any_column_privilege( \
                current_user, relation.oid, checked_privilege.name) \
              ) AS caller_has_column_privilege, \
              EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.pg_auth_members AS membership \
               WHERE membership.member = caller_role.oid \
                OR membership.roleid = caller_role.oid \
              ) AS caller_has_role_membership, \
              COALESCE( \
               caller_role.rolsuper OR caller_role.rolcreatedb \
                OR caller_role.rolcreaterole OR caller_role.rolreplication \
                OR caller_role.rolbypassrls, TRUE \
              ) AS caller_role_excessive, \
              COALESCE(pg_catalog.pg_has_role( \
               current_user, owner_role.rolname, 'MEMBER'), TRUE) \
                AS caller_is_owner_member \
             FROM target \
             CROSS JOIN function_contract \
             CROSS JOIN relation_contract \
             LEFT JOIN caller_role ON TRUE \
             LEFT JOIN owner_role ON TRUE",
        )
        .bind(FUNCTION_IDENTITY)
        .bind(FUNCTION_RESULT)
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        if let Err(error) = contract.verify() {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(error);
        }
        let probe_rows = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_installation_authority_read_v1($1, $2, $3)",
        )
        .bind(PROBE_IDENTITY)
        .bind(PROBE_IDENTITY)
        .bind([0_u8; 32].as_slice())
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        if probe_rows != 0 {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(InstallationAuthorityReadinessErrorV1::ContractMismatch);
        }
        transaction.commit().await.map_err(readiness_database)?;
        Ok(())
    }
}

fn readiness_database(error: sqlx::Error) -> InstallationAuthorityReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_contract() -> InstallationAuthorityReadinessRow {
        InstallationAuthorityReadinessRow {
            function_contract_valid: true,
            owner_contract_valid: true,
            public_execute_revoked: true,
            caller_session_direct: true,
            caller_execute: true,
            caller_execute_grantable: false,
            unexpected_function_grant: false,
            caller_database_connect: true,
            caller_database_create: false,
            caller_database_temporary: false,
            caller_schema_usage: true,
            caller_schema_create: false,
            caller_has_table_privilege: false,
            caller_has_column_privilege: false,
            caller_has_role_membership: false,
            caller_role_excessive: false,
            caller_is_owner_member: false,
        }
    }

    #[test]
    fn readiness_contract_separates_missing_and_excess_capabilities() {
        assert_eq!(valid_contract().verify(), Ok(()));
        let mut missing = valid_contract();
        missing.caller_execute = false;
        assert_eq!(
            missing.verify(),
            Err(InstallationAuthorityReadinessErrorV1::CapabilityMissing)
        );
        let mut excessive = valid_contract();
        excessive.caller_database_temporary = true;
        assert_eq!(
            excessive.verify(),
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        let mut invalid = valid_contract();
        invalid.owner_contract_valid = false;
        assert_eq!(
            invalid.verify(),
            Err(InstallationAuthorityReadinessErrorV1::ContractMismatch)
        );
        let mut indirect = valid_contract();
        indirect.caller_session_direct = false;
        assert_eq!(
            indirect.verify(),
            Err(InstallationAuthorityReadinessErrorV1::ContractMismatch)
        );
        let mut unexpected_grant = valid_contract();
        unexpected_grant.unexpected_function_grant = true;
        assert_eq!(
            unexpected_grant.verify(),
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        let mut membership = valid_contract();
        membership.caller_has_role_membership = true;
        assert_eq!(
            membership.verify(),
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
    }
}
