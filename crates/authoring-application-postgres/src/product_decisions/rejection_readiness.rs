use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseProbeModeV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::product_action_digest::product_action_keyring_coverage_identity_v1;
use crate::ProductDecisionReadinessErrorV1;

use super::readiness::{map_readiness, readiness_database, verify_approval_support_contract};
use super::reject::PostgresProductRejections;
use super::rejection_contract::{
    DATABASE_IDENTITY_FUNCTION, KEYRING_COVERAGE_ARGUMENTS, KEYRING_COVERAGE_FUNCTION,
    KEYRING_COVERAGE_RESULT, REJECT_ARGUMENTS, REJECT_FUNCTION, REJECT_RESULT, TOPOLOGY_QUERY,
};

const REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.rejection.digest-key-fingerprint.v1";
const FUNCTIONS: [ScopedFunctionContractV1<'static>; 3] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_plpgsql_named(
        KEYRING_COVERAGE_FUNCTION,
        KEYRING_COVERAGE_RESULT,
        1.0,
        KEYRING_COVERAGE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        REJECT_FUNCTION,
        REJECT_RESULT,
        1.0,
        REJECT_ARGUMENTS,
    ),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 16] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_promotions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipts"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.product_action_receipt_idempotency_aliases",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.product_audit_events"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipt_audit_evidence"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_request_approvals"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_activations"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_versions"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_deployments"),
];
const PROBE_SESSION_DIGEST: [u8; 32] = [61_u8; 32];
const PROBE_SUBJECT_DIGEST: [u8; 32] = [109_u8; 32];
const REJECTION_PROBE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, \
    exact_replay, guild_id FROM public.starring_product_reject_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    pg_catalog.repeat('7', 64), pg_catalog.repeat('8', 64), 'probe reason') LIMIT 2";

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct RejectionProbeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
}

impl PostgresProductRejections {
    pub async fn verify_product_rejection_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_product_rejection_readiness().await.map(drop)
    }

    pub(super) async fn check_product_rejection_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut metadata = begin_scoped_database_readiness(
            &self.rejection_executor,
            &timeout,
            &FUNCTIONS,
            &RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        verify_scoped_executable_allowlist(&mut metadata, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_global_user_object_deny(&mut metadata, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_schema_trust(&mut metadata, "public", DATABASE_IDENTITY_FUNCTION)
            .await
            .map_err(map_readiness)?;
        verify_approval_support_contract(&mut metadata).await?;
        let topology = load_scoped_database_topology(&mut metadata, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        if let Err(error) = self.check_rejection_keyring_coverage(&mut metadata).await {
            metadata.rollback().await.map_err(readiness_database)?;
            return Err(error);
        }
        metadata.commit().await.map_err(readiness_database)?;
        self.run_rejection_probe().await?;
        Ok(topology)
    }

    async fn check_rejection_keyring_coverage(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = product_action_keyring_coverage_identity_v1(
            self.config.keyring(),
            REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN,
        );
        let outcomes = sqlx::query_scalar::<_, String>(
            "SELECT outcome \
             FROM public.starring_product_rejection_keyring_coverage_v1($1, $2) LIMIT 2",
        )
        .bind(&identity.key_ids)
        .bind(&identity.key_fingerprints)
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
        match outcomes.as_slice() {
            [outcome] if outcome == "ok" => Ok(()),
            [outcome] if outcome == "idempotency_keyring_incomplete" => {
                Err(ProductDecisionReadinessErrorV1::IncompleteCoverage)
            }
            _ => Err(ProductDecisionReadinessErrorV1::InvalidResult),
        }
    }

    async fn run_rejection_probe(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let mut transaction = begin_bounded_database_probe(
            &self.rejection_executor,
            &self.config.statement_timeout(),
            ScopedDatabaseProbeModeV1::SerializableReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let result = run_rejection_probe(&mut transaction).await;
        transaction.rollback().await.map_err(readiness_database)?;
        result
    }
}

async fn run_rejection_probe(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let rows = sqlx::query_as::<_, RejectionProbeRow>(REJECTION_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if rows
        != [RejectionProbeRow {
            outcome: "invalid_input".to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
        }]
    {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_manifest_is_exact_and_nonempty() {
        assert_eq!(FUNCTIONS.len(), 3);
        assert_eq!(RELATIONS.len(), 16);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 32);
        assert_eq!(PROBE_SUBJECT_DIGEST.len(), 32);
    }
}
