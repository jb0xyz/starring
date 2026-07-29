use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness,
    load_scoped_database_session_identity, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseProbeModeV1, ScopedDatabaseReadinessErrorV1,
    ScopedDatabaseSessionIdentityV1, ScopedDatabaseTopologyV1, ScopedFunctionContractV1,
    ScopedRelationContractV1,
};
use crate::envelope::SnapshotEnvelopeCipher;
use crate::ProductDatabaseFailureV1;

use super::digest::writer_keyring_coverage_identity_v1;
use super::row::AuthoringWriterCoverageRowV1;
use super::store::PostgresAuthoringConversationStoreV1;

const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_authoring_session_writer_database_identity_v1()";
const CHECK_FUNCTION: &str = "public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])";
const COMMIT_FUNCTION: &str = "public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)";
const LOAD_FUNCTION: &str =
    "public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)";
const KEY_COVERAGE_FUNCTION: &str =
    "public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])";
const CHECK_RESULT: &str = "TABLE(outcome_code text, current_generation bigint, matched_generation bigint, safe_turn_projection bytea, safe_turn_projection_digest text)";
const COMMIT_RESULT: &str = "TABLE(outcome_code text, current_generation bigint, committed_generation bigint, safe_turn_projection bytea, safe_turn_projection_digest text)";
const LOAD_RESULT: &str = "TABLE(outcome_code text, head_generation bigint, snapshot_schema_version bigint, snapshot_ciphertext bytea, snapshot_nonce bytea, encryption_key_id text, encryption_suite text, encryption_suite_version smallint, authenticated_metadata_digest text, resource_bindings jsonb, binding_fingerprint text, installation_authority_revision bigint, authority_payload_digest text, writer_request_digest text, writer_semantic_request_digest text, writer_digest_key_id text, writer_digest_key_fingerprint text, safe_turn_projection bytea, safe_turn_projection_digest text, stage text, candidate_revision bigint, candidate_hash text, harness_contract_revision bigint, current_authority_revision bigint, current_authority_payload_digest text, current_resource_bindings jsonb, current_binding_fingerprint text)";
const KEY_COVERAGE_RESULT: &str = "TABLE(covered boolean)";
const TOPOLOGY_QUERY: &str = "SELECT \
    public.starring_authoring_session_writer_database_identity_v1(), \
    current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const FUNCTIONS: [ScopedFunctionContractV1<'static>; 5] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_plpgsql(CHECK_FUNCTION, CHECK_RESULT, 1.0),
    ScopedFunctionContractV1::set_plpgsql_non_strict_trusted_public(
        COMMIT_FUNCTION,
        COMMIT_RESULT,
        1.0,
    ),
    ScopedFunctionContractV1::set_plpgsql(LOAD_FUNCTION, LOAD_RESULT, 1.0),
    ScopedFunctionContractV1::set_plpgsql(KEY_COVERAGE_FUNCTION, KEY_COVERAGE_RESULT, 1.0),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 7] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_session_generations"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringConversationStoreReadinessErrorV1 {
    #[error("authoring writer database contract is invalid")]
    ContractMismatch,
    #[error("authoring writer database capability is missing")]
    CapabilityMissing,
    #[error("authoring writer database capability is excessive")]
    ExcessCapability,
    #[error("authoring writer key coverage is incomplete")]
    IncompleteKeyCoverage,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl<C: SnapshotEnvelopeCipher> PostgresAuthoringConversationStoreV1<C> {
    pub async fn verify_readiness(&self) -> Result<(), AuthoringConversationStoreReadinessErrorV1> {
        self.check_readiness().await.map(|_| ())
    }

    pub(crate) async fn check_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, AuthoringConversationStoreReadinessErrorV1> {
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
        let encryption_key_ids = self
            .cipher
            .configured_encryption_key_ids()
            .ok_or(AuthoringConversationStoreReadinessErrorV1::IncompleteKeyCoverage)?
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let digest_identity = writer_keyring_coverage_identity_v1(&self.digest_keyring);
        let coverage = sqlx::query_as::<_, AuthoringWriterCoverageRowV1>(
            "SELECT * \
             FROM public.starring_authoring_session_writer_key_coverage_v1($1, $2, $3)",
        )
        .bind(encryption_key_ids)
        .bind(digest_identity.key_ids)
        .bind(digest_identity.key_fingerprints)
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        if !coverage.covered {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(AuthoringConversationStoreReadinessErrorV1::IncompleteKeyCoverage);
        }
        transaction.commit().await.map_err(readiness_database)?;
        Ok(topology)
    }

    pub(crate) async fn check_session_identity(
        &self,
    ) -> Result<ScopedDatabaseSessionIdentityV1, AuthoringConversationStoreReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut transaction =
            begin_bounded_database_probe(&self.pool, &timeout, ScopedDatabaseProbeModeV1::ReadOnly)
                .await
                .map_err(map_readiness)?;
        let identity = load_scoped_database_session_identity(&mut transaction)
            .await
            .map_err(map_readiness)?;
        transaction.commit().await.map_err(readiness_database)?;
        Ok(identity)
    }
}

fn map_readiness(
    error: ScopedDatabaseReadinessErrorV1,
) -> AuthoringConversationStoreReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            AuthoringConversationStoreReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            AuthoringConversationStoreReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            AuthoringConversationStoreReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> AuthoringConversationStoreReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readiness_errors_keep_the_writer_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            AuthoringConversationStoreReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            AuthoringConversationStoreReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            AuthoringConversationStoreReadinessErrorV1::ExcessCapability
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::Database(
                ProductDatabaseFailureV1::Timeout,
            )),
            AuthoringConversationStoreReadinessErrorV1::Database(ProductDatabaseFailureV1::Timeout)
        );
    }
}
