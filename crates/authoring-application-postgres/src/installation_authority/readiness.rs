use super::PostgresInstallationAuthoritySource;
use crate::database_capability::{
    begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseReadinessErrorV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::ProductDatabaseFailureV1;

const FUNCTION_IDENTITY: &str =
    "public.starring_product_installation_authority_read_v1(text,text,bytea)";
const FUNCTION_RESULT: &str = "TABLE(principal_id text, acting_user_id text, principal_disabled boolean, session_digest bytea, session_principal_id text, oauth_state_digest_length integer, last_seen_at timestamp with time zone, idle_expires_at timestamp with time zone, absolute_expires_at timestamp with time zone, revoked_at timestamp with time zone, installation_tenant_id text, installation_id text, tenant_id text, tenant_lifecycle_state text, installation_lifecycle_state text, discord_application_id text, discord_guild_id text, current_authority_revision bigint, authority_tenant_id text, authority_installation_id text, authority_revision bigint, authority_payload_digest text, database_now timestamp with time zone)";
const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_installation_authority_reader_database_identity_v1()";
const TOPOLOGY_QUERY: &str = "SELECT \
    public.starring_product_installation_authority_reader_database_identity_v1(), \
    current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const PROBE_IDENTITY: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const FUNCTIONS: [ScopedFunctionContractV1<'static>; 2] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set(FUNCTION_IDENTITY, FUNCTION_RESULT, 1.0),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 6] = [
    ScopedRelationContractV1::ordinary("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
];

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

impl PostgresInstallationAuthoritySource {
    pub async fn verify_readiness(&self) -> Result<(), InstallationAuthorityReadinessErrorV1> {
        self.check_readiness().await.map(|_| ())
    }

    pub(crate) async fn check_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, InstallationAuthorityReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut transaction =
            begin_scoped_database_readiness(&self.pool, &timeout, &FUNCTIONS, &RELATIONS)
                .await
                .map_err(map_readiness)?;
        verify_scoped_executable_allowlist(&mut transaction, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_global_user_object_deny(&mut transaction, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_schema_trust(&mut transaction, "public", DATABASE_IDENTITY_FUNCTION)
            .await
            .map_err(map_readiness)?;
        let topology = load_scoped_database_topology(&mut transaction, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
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
        Ok(topology)
    }
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> InstallationAuthorityReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            InstallationAuthorityReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            InstallationAuthorityReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            InstallationAuthorityReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> InstallationAuthorityReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readiness_errors_keep_the_public_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            InstallationAuthorityReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            InstallationAuthorityReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            InstallationAuthorityReadinessErrorV1::ExcessCapability
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::Database(
                ProductDatabaseFailureV1::Timeout,
            )),
            InstallationAuthorityReadinessErrorV1::Database(ProductDatabaseFailureV1::Timeout,)
        );
    }
}
