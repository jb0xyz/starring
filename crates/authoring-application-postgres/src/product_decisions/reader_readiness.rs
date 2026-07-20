use crate::database_capability::{
    begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseReadinessErrorV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::{ProductDatabaseFailureV1, ProductDecisionReadinessErrorV1};

use super::reader_contract::{
    DATABASE_IDENTITY_FUNCTION, READ_FUNCTION, READ_RESULT, TOPOLOGY_QUERY,
};
use super::store::PostgresProductDecisions;

const FUNCTIONS: [ScopedFunctionContractV1<'static>; 2] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set(READ_FUNCTION, READ_RESULT, 1.0),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 12] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_request_approvals"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_promotions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_session_generations"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_deployments"),
];
const PROBE_QUERY: &str = "SELECT pg_catalog.count(*) \
     FROM public.starring_product_decision_read_v1( \
      pg_catalog.repeat('0', 64), 'probe_tenant', 'probe_installation', \
      '18446744073709551615', 'probe_principal', '18446744073709551615', $1)";
const PROBE_SESSION_DIGEST: [u8; 32] = [67_u8; 32];

impl PostgresProductDecisions {
    pub async fn verify_decision_reader_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_decision_reader_readiness().await.map(drop)
    }

    pub(super) async fn check_decision_reader_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
        let mut transaction = begin_scoped_database_readiness(
            &self.pools.decision_reader,
            &self.config.statement_timeout(),
            &FUNCTIONS,
            &RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        verify_scoped_executable_allowlist(&mut transaction, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_global_user_object_deny(&mut transaction, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_schema_trust(&mut transaction, "public", READ_FUNCTION)
            .await
            .map_err(map_readiness)?;
        let topology = load_scoped_database_topology(&mut transaction, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        let probe_rows = sqlx::query_scalar::<_, i64>(PROBE_QUERY)
            .bind(PROBE_SESSION_DIGEST.as_slice())
            .fetch_one(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        if probe_rows != 0 {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
        }
        transaction.commit().await.map_err(readiness_database)?;
        Ok(topology)
    }
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> ProductDecisionReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            ProductDecisionReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            ProductDecisionReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            ProductDecisionReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> ProductDecisionReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_manifest_is_exact_and_nonempty() {
        assert_eq!(FUNCTIONS.len(), 2);
        assert_eq!(RELATIONS.len(), 12);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 32);
    }
}
