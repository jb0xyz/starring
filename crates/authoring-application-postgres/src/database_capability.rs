use sqlx::postgres::PgPool;
use sqlx::{Postgres, Transaction};

use crate::ProductDatabaseFailureV1;

#[derive(Clone, Copy)]
pub(crate) struct ScopedFunctionContractV1<'a> {
    identity: &'a str,
    result: &'a str,
    returns_set: bool,
    rows: f32,
}

impl<'a> ScopedFunctionContractV1<'a> {
    pub(crate) const fn set(identity: &'a str, result: &'a str, rows: f32) -> Self {
        Self {
            identity,
            result,
            returns_set: true,
            rows,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScopedRelationContractV1<'a> {
    identity: &'a str,
    require_rls_disabled: bool,
}

impl<'a> ScopedRelationContractV1<'a> {
    pub(crate) const fn ordinary(identity: &'a str) -> Self {
        Self {
            identity,
            require_rls_disabled: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopedDatabaseReadinessErrorV1 {
    ContractMismatch,
    CapabilityMissing,
    ExcessCapability,
    Database(ProductDatabaseFailureV1),
}

#[derive(sqlx::FromRow)]
struct FunctionCapabilityRow {
    metadata_valid: bool,
    owner_name: Option<String>,
    public_execute_revoked: bool,
    caller_execute: bool,
    caller_execute_grantable: bool,
    unexpected_function_grant: bool,
}

#[derive(sqlx::FromRow)]
struct RelationCapabilityRow {
    ordinary_table: bool,
    rls_disabled: bool,
    owner_name: Option<String>,
    caller_has_table_privilege: bool,
    caller_has_column_privilege: bool,
}

#[derive(sqlx::FromRow)]
struct RoleCapabilityRow {
    owner_contract_valid: bool,
    caller_session_direct: bool,
    caller_database_connect: bool,
    caller_database_create: bool,
    caller_database_temporary: bool,
    caller_schema_usage: bool,
    caller_schema_create: bool,
    caller_has_role_membership: bool,
    caller_role_excessive: bool,
    caller_is_owner_member: bool,
}

struct ScopedDatabaseCapabilitiesV1 {
    contract_valid: bool,
    capability_present: bool,
    excess_capability: bool,
    owner_name: Option<String>,
}

impl ScopedDatabaseCapabilitiesV1 {
    fn new() -> Self {
        Self {
            contract_valid: true,
            capability_present: true,
            excess_capability: false,
            owner_name: None,
        }
    }

    fn observe_owner(&mut self, owner_name: Option<String>) {
        let Some(owner_name) = owner_name else {
            self.contract_valid = false;
            return;
        };
        match self.owner_name.as_deref() {
            Some(expected) if expected != owner_name => self.contract_valid = false,
            Some(_) => {}
            None => self.owner_name = Some(owner_name),
        }
    }

    fn observe_function(&mut self, row: FunctionCapabilityRow) {
        self.contract_valid &= row.metadata_valid && row.public_execute_revoked;
        self.capability_present &= row.caller_execute;
        self.excess_capability |= row.caller_execute_grantable || row.unexpected_function_grant;
        self.observe_owner(row.owner_name);
    }

    fn observe_relation(&mut self, row: RelationCapabilityRow, require_rls_disabled: bool) {
        self.contract_valid &= row.ordinary_table;
        if require_rls_disabled {
            self.contract_valid &= row.rls_disabled;
        }
        self.excess_capability |= row.caller_has_table_privilege || row.caller_has_column_privilege;
        self.observe_owner(row.owner_name);
    }

    fn observe_role(&mut self, row: RoleCapabilityRow) {
        self.contract_valid &= row.owner_contract_valid && row.caller_session_direct;
        self.capability_present &= row.caller_database_connect && row.caller_schema_usage;
        self.excess_capability |= row.caller_database_create
            || row.caller_database_temporary
            || row.caller_schema_create
            || row.caller_has_role_membership
            || row.caller_role_excessive
            || row.caller_is_owner_member;
    }

    fn verify(self) -> Result<(), ScopedDatabaseReadinessErrorV1> {
        if !self.contract_valid || self.owner_name.is_none() {
            return Err(ScopedDatabaseReadinessErrorV1::ContractMismatch);
        }
        if !self.capability_present {
            return Err(ScopedDatabaseReadinessErrorV1::CapabilityMissing);
        }
        if self.excess_capability {
            return Err(ScopedDatabaseReadinessErrorV1::ExcessCapability);
        }
        Ok(())
    }
}

pub(crate) async fn begin_scoped_database_readiness<'a>(
    pool: &'a PgPool,
    timeout: &str,
    functions: &[ScopedFunctionContractV1<'_>],
    relations: &[ScopedRelationContractV1<'_>],
) -> Result<Transaction<'a, Postgres>, ScopedDatabaseReadinessErrorV1> {
    let mut transaction = pool.begin().await.map_err(readiness_database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(readiness_database)?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $1, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
    )
    .bind(timeout)
    .execute(&mut *transaction)
    .await
    .map_err(readiness_database)?;

    let mut capabilities = ScopedDatabaseCapabilitiesV1::new();
    for function in functions {
        let row = load_function_capability(&mut transaction, function).await?;
        capabilities.observe_function(row);
    }
    for relation in relations {
        let row = load_relation_capability(&mut transaction, relation.identity).await?;
        capabilities.observe_relation(row, relation.require_rls_disabled);
    }
    let owner_name = capabilities.owner_name.clone();
    if let Some(owner_name) = owner_name {
        let row = load_role_capability(&mut transaction, &owner_name).await?;
        capabilities.observe_role(row);
    }
    if let Err(error) = capabilities.verify() {
        transaction.rollback().await.map_err(readiness_database)?;
        return Err(error);
    }
    Ok(transaction)
}

async fn load_function_capability(
    transaction: &mut Transaction<'_, Postgres>,
    contract: &ScopedFunctionContractV1<'_>,
) -> Result<FunctionCapabilityRow, ScopedDatabaseReadinessErrorV1> {
    sqlx::query_as::<_, FunctionCapabilityRow>(
        "WITH target AS ( \
           SELECT pg_catalog.to_regprocedure($1) AS function_oid \
         ), function_contract AS ( \
           SELECT function_row.oid, function_row.proowner, function_row.proacl, \
            function_row.prosecdef, function_row.proisstrict, function_row.provolatile, \
            function_row.proparallel, function_row.proretset, function_row.prorows, \
            function_row.proconfig, function_row.prokind, language.lanname, \
            pg_catalog.pg_get_function_result(function_row.oid) AS function_result \
           FROM target \
           LEFT JOIN pg_catalog.pg_proc AS function_row \
            ON function_row.oid = target.function_oid \
           LEFT JOIN pg_catalog.pg_language AS language \
            ON language.oid = function_row.prolang \
         ), caller_role AS ( \
           SELECT role.oid FROM pg_catalog.pg_roles AS role \
           WHERE role.rolname = current_user \
         ) \
         SELECT COALESCE( \
           function_contract.oid IS NOT NULL \
            AND function_contract.prokind = 'f' \
            AND function_contract.prosecdef \
            AND function_contract.proisstrict \
            AND function_contract.provolatile = 'v' \
            AND function_contract.proparallel = 'u' \
            AND function_contract.proretset = $3 \
            AND function_contract.prorows = $4 \
            AND function_contract.proconfig = ARRAY['search_path=pg_catalog']::TEXT[] \
            AND function_contract.lanname = 'sql' \
            AND function_contract.function_result = $2, FALSE \
          ) AS metadata_valid, \
          pg_catalog.pg_get_userbyid(function_contract.proowner)::TEXT AS owner_name, \
          COALESCE(NOT EXISTS ( \
           SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
            function_contract.proacl, \
            pg_catalog.acldefault('f', function_contract.proowner) \
           )) AS privilege \
           WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE' \
          ), FALSE) AS public_execute_revoked, \
          COALESCE(EXISTS ( \
           SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
            function_contract.proacl, \
            pg_catalog.acldefault('f', function_contract.proowner) \
           )) AS privilege \
           WHERE privilege.grantee = caller_role.oid \
            AND privilege.privilege_type = 'EXECUTE' \
          ), FALSE) AS caller_execute, \
          COALESCE(EXISTS ( \
           SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
            function_contract.proacl, \
            pg_catalog.acldefault('f', function_contract.proowner) \
           )) AS privilege \
           WHERE privilege.grantee = caller_role.oid \
            AND privilege.privilege_type = 'EXECUTE' \
            AND privilege.is_grantable \
          ), FALSE) AS caller_execute_grantable, \
          COALESCE(EXISTS ( \
           SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
            function_contract.proacl, \
            pg_catalog.acldefault('f', function_contract.proowner) \
           )) AS privilege \
           WHERE privilege.grantee <> 0 \
            AND privilege.grantee <> function_contract.proowner \
            AND privilege.grantee <> caller_role.oid \
            AND privilege.privilege_type = 'EXECUTE' \
          ), TRUE) AS unexpected_function_grant \
         FROM function_contract \
         LEFT JOIN caller_role ON TRUE",
    )
    .bind(contract.identity)
    .bind(contract.result)
    .bind(contract.returns_set)
    .bind(contract.rows)
    .fetch_one(&mut **transaction)
    .await
    .map_err(readiness_database)
}

async fn load_relation_capability(
    transaction: &mut Transaction<'_, Postgres>,
    identity: &str,
) -> Result<RelationCapabilityRow, ScopedDatabaseReadinessErrorV1> {
    sqlx::query_as::<_, RelationCapabilityRow>(
        "WITH target AS ( \
           SELECT pg_catalog.to_regclass($1) AS relation_oid \
         ) \
         SELECT COALESCE(relation.relkind = 'r', FALSE) AS ordinary_table, \
          COALESCE(NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity, FALSE) \
           AS rls_disabled, \
          pg_catalog.pg_get_userbyid(relation.relowner)::TEXT AS owner_name, \
          COALESCE(EXISTS ( \
           SELECT 1 FROM (VALUES \
            ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'), \
            ('TRUNCATE'), ('REFERENCES'), ('TRIGGER') \
           ) AS checked_privilege(name) \
           WHERE pg_catalog.has_table_privilege( \
            current_user, relation.oid, checked_privilege.name) \
          ), FALSE) AS caller_has_table_privilege, \
          COALESCE(EXISTS ( \
           SELECT 1 FROM (VALUES \
            ('SELECT'), ('INSERT'), ('UPDATE'), ('REFERENCES') \
           ) AS checked_privilege(name) \
           WHERE pg_catalog.has_any_column_privilege( \
            current_user, relation.oid, checked_privilege.name) \
          ), FALSE) AS caller_has_column_privilege \
         FROM target \
         LEFT JOIN pg_catalog.pg_class AS relation \
          ON relation.oid = target.relation_oid",
    )
    .bind(identity)
    .fetch_one(&mut **transaction)
    .await
    .map_err(readiness_database)
}

async fn load_role_capability(
    transaction: &mut Transaction<'_, Postgres>,
    owner_name: &str,
) -> Result<RoleCapabilityRow, ScopedDatabaseReadinessErrorV1> {
    sqlx::query_as::<_, RoleCapabilityRow>(
        "WITH target AS ( \
           SELECT pg_catalog.to_regnamespace('public') AS schema_oid \
         ), caller_role AS ( \
           SELECT role.oid, role.rolsuper, role.rolcreatedb, role.rolcreaterole, \
            role.rolcanlogin, role.rolreplication, role.rolbypassrls \
           FROM pg_catalog.pg_roles AS role WHERE role.rolname = current_user \
         ), owner_role AS ( \
           SELECT role.oid, role.rolname, role.rolsuper, role.rolcreatedb, \
            role.rolcreaterole, role.rolcanlogin, role.rolreplication, \
            role.rolbypassrls \
           FROM pg_catalog.pg_roles AS role WHERE role.rolname = $1 \
         ) \
         SELECT COALESCE( \
           NOT owner_role.rolsuper AND NOT owner_role.rolcreatedb \
            AND NOT owner_role.rolcreaterole AND NOT owner_role.rolcanlogin \
            AND NOT owner_role.rolreplication AND NOT owner_role.rolbypassrls \
            AND pg_catalog.has_schema_privilege( \
             owner_role.rolname, target.schema_oid, 'USAGE') \
            AND NOT EXISTS ( \
             SELECT 1 FROM pg_catalog.pg_auth_members AS membership \
             WHERE membership.member = owner_role.oid \
              OR membership.roleid = owner_role.oid \
            ), FALSE \
          ) AS owner_contract_valid, \
          COALESCE(current_user = session_user AND caller_role.rolcanlogin, FALSE) \
           AS caller_session_direct, \
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
           current_user, target.schema_oid, 'USAGE'), FALSE) AS caller_schema_usage, \
          COALESCE(pg_catalog.has_schema_privilege( \
           current_user, target.schema_oid, 'CREATE'), FALSE) AS caller_schema_create, \
          COALESCE(EXISTS ( \
           SELECT 1 FROM pg_catalog.pg_auth_members AS membership \
           WHERE membership.member = caller_role.oid \
            OR membership.roleid = caller_role.oid \
          ), TRUE) AS caller_has_role_membership, \
          COALESCE( \
           caller_role.rolsuper OR caller_role.rolcreatedb \
            OR caller_role.rolcreaterole OR caller_role.rolreplication \
            OR caller_role.rolbypassrls, TRUE \
          ) AS caller_role_excessive, \
          COALESCE(pg_catalog.pg_has_role( \
           current_user, owner_role.rolname, 'MEMBER'), TRUE) \
           AS caller_is_owner_member \
         FROM target \
         LEFT JOIN caller_role ON TRUE \
         LEFT JOIN owner_role ON TRUE",
    )
    .bind(owner_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(readiness_database)
}

fn readiness_database(error: sqlx::Error) -> ScopedDatabaseReadinessErrorV1 {
    ScopedDatabaseReadinessErrorV1::Database(ProductDatabaseFailureV1::classify(&error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_capabilities() -> ScopedDatabaseCapabilitiesV1 {
        ScopedDatabaseCapabilitiesV1 {
            contract_valid: true,
            capability_present: true,
            excess_capability: false,
            owner_name: Some("starring_owner".to_string()),
        }
    }

    #[test]
    fn readiness_result_prioritizes_contract_then_missing_then_excess() {
        assert_eq!(valid_capabilities().verify(), Ok(()));
        let mut invalid = valid_capabilities();
        invalid.contract_valid = false;
        invalid.capability_present = false;
        invalid.excess_capability = true;
        assert_eq!(
            invalid.verify(),
            Err(ScopedDatabaseReadinessErrorV1::ContractMismatch)
        );
        let mut missing = valid_capabilities();
        missing.capability_present = false;
        missing.excess_capability = true;
        assert_eq!(
            missing.verify(),
            Err(ScopedDatabaseReadinessErrorV1::CapabilityMissing)
        );
        let mut excessive = valid_capabilities();
        excessive.excess_capability = true;
        assert_eq!(
            excessive.verify(),
            Err(ScopedDatabaseReadinessErrorV1::ExcessCapability)
        );
    }

    #[test]
    fn readiness_rejects_missing_or_divergent_owners() {
        let mut missing = valid_capabilities();
        missing.observe_owner(None);
        assert_eq!(
            missing.verify(),
            Err(ScopedDatabaseReadinessErrorV1::ContractMismatch)
        );
        let mut divergent = valid_capabilities();
        divergent.observe_owner(Some("other_owner".to_string()));
        assert_eq!(
            divergent.verify(),
            Err(ScopedDatabaseReadinessErrorV1::ContractMismatch)
        );
    }
}
