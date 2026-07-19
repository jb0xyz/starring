use super::PostgresAuthorizedPromotionSnapshots;
use crate::database_capability::{
    begin_scoped_database_readiness, ScopedDatabaseReadinessErrorV1, ScopedFunctionContractV1,
    ScopedRelationContractV1,
};
use crate::ProductDatabaseFailureV1;

const FUNCTION_IDENTITY: &str =
    "public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)";
const FUNCTION_RESULT: &str = "TABLE(session_tenant_id text, session_installation_id text, owner_principal_id text, owner_discord_user_id text, owner_disabled boolean, actor_session_digest bytea, current_generation bigint, session_lifecycle_state text, tenant_lifecycle_state text, installation_tenant_id text, discord_application_id text, discord_guild_id text, ruleset_key text, installation_lifecycle_state text, current_authority_revision bigint, generation bigint, snapshot_schema_version bigint, snapshot_ciphertext bytea, snapshot_nonce bytea, encryption_key_id text, encryption_suite text, encryption_suite_version smallint, authenticated_metadata_digest text, generation_resource_bindings jsonb, generation_binding_fingerprint text, installation_authority_revision bigint, generation_stage text, candidate_revision bigint, candidate_hash text, harness_contract_revision bigint, authority_tenant_id text, binding_revision bigint, authority_resource_bindings jsonb, authority_binding_fingerprint text, policy_revision bigint, required_approvals integer, activation_ttl_seconds bigint, authority_payload_digest text, database_now timestamp with time zone)";
const PROBE_IDENTITY: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const PROBE_DIGEST: [u8; 31] = [0_u8; 31];
const FUNCTIONS: [ScopedFunctionContractV1<'static>; 1] = [ScopedFunctionContractV1::set(
    FUNCTION_IDENTITY,
    FUNCTION_RESULT,
    1.0,
)];
const RELATIONS: [ScopedRelationContractV1<'static>; 7] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_session_generations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizedSnapshotReadinessErrorV1 {
    #[error("authorized snapshot database contract is invalid")]
    ContractMismatch,
    #[error("authorized snapshot database capability is missing")]
    CapabilityMissing,
    #[error("authorized snapshot database capability is excessive")]
    ExcessCapability,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl<C> PostgresAuthorizedPromotionSnapshots<C> {
    pub async fn verify_readiness(&self) -> Result<(), AuthorizedSnapshotReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut transaction =
            begin_scoped_database_readiness(&self.pool, &timeout, &FUNCTIONS, &RELATIONS)
                .await
                .map_err(map_readiness)?;
        let probe_rows = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_authorized_snapshot_read_v1(\
              $1, $2, $3, $4, $5)",
        )
        .bind(PROBE_IDENTITY)
        .bind(PROBE_IDENTITY)
        .bind(PROBE_DIGEST.as_slice())
        .bind(PROBE_IDENTITY)
        .bind(PROBE_IDENTITY)
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        if probe_rows != 0 {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(AuthorizedSnapshotReadinessErrorV1::ContractMismatch);
        }
        transaction.commit().await.map_err(readiness_database)?;
        Ok(())
    }
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> AuthorizedSnapshotReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            AuthorizedSnapshotReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            AuthorizedSnapshotReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            AuthorizedSnapshotReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> AuthorizedSnapshotReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readiness_errors_keep_the_snapshot_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            AuthorizedSnapshotReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            AuthorizedSnapshotReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            AuthorizedSnapshotReadinessErrorV1::ExcessCapability
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::Database(
                ProductDatabaseFailureV1::Timeout,
            )),
            AuthorizedSnapshotReadinessErrorV1::Database(ProductDatabaseFailureV1::Timeout)
        );
    }
}
