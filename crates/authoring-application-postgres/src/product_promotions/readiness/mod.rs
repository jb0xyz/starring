use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_schema_trust, ScopedDatabaseProbeModeV1,
    ScopedDatabaseReadinessErrorV1, ScopedDatabaseTopologyV1,
};
use crate::product_action_digest::product_action_keyring_coverage_identity_v1;
use crate::ProductDatabaseFailureV1;

use super::store::PostgresProductPromotions;

mod manifest;
mod metadata;
mod probes;

use manifest::*;
use metadata::SUPPORT_CONTRACT_QUERY;
use probes::run_hostile_probes;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductPromotionReadinessErrorV1 {
    #[error("product promotion database contract is invalid")]
    ContractMismatch,
    #[error("product promotion database capability is missing")]
    CapabilityMissing,
    #[error("product promotion database capability is excessive")]
    ExcessCapability,
    #[error("product promotion keyring does not cover durable state")]
    IncompleteCoverage,
    #[error("product promotion readiness returned an invalid result")]
    InvalidProbeResult,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl PostgresProductPromotions {
    pub async fn verify_readiness(&self) -> Result<(), ProductPromotionReadinessErrorV1> {
        self.check_readiness().await.map(drop)
    }

    pub(crate) async fn check_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductPromotionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut metadata =
            begin_scoped_database_readiness(&self.executor, &timeout, &FUNCTIONS, &RELATIONS)
                .await
                .map_err(map_readiness)?;
        let result = self.check_metadata(&mut metadata).await;
        metadata.rollback().await.map_err(readiness_database)?;
        let topology = result?;
        self.run_hostile_probes().await?;
        Ok(topology)
    }

    async fn check_metadata(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<ScopedDatabaseTopologyV1, ProductPromotionReadinessErrorV1> {
        verify_scoped_executable_allowlist(transaction, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_schema_trust(transaction, "public", DATABASE_IDENTITY_FUNCTION)
            .await
            .map_err(map_readiness)?;
        let support_valid = sqlx::query_scalar::<_, bool>(SUPPORT_CONTRACT_QUERY)
            .fetch_one(&mut **transaction)
            .await
            .map_err(readiness_database)?;
        if !support_valid {
            return Err(ProductPromotionReadinessErrorV1::ContractMismatch);
        }
        let topology = load_scoped_database_topology(transaction, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        self.check_keyring_coverage(transaction).await?;
        Ok(topology)
    }

    async fn check_keyring_coverage(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProductPromotionReadinessErrorV1> {
        let identity = product_action_keyring_coverage_identity_v1(
            self.config.keyring(),
            KEY_MATERIAL_FINGERPRINT_DOMAIN,
        );
        let outcomes = sqlx::query_scalar::<_, String>(
            "SELECT outcome_code \
             FROM public.starring_product_promotion_keyring_coverage_v1($1, $2) \
             LIMIT 2",
        )
        .bind(&identity.key_ids)
        .bind(&identity.key_fingerprints)
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
        match outcomes.as_slice() {
            [outcome] if outcome == "covered" => Ok(()),
            [outcome] if outcome == "missing_key" => {
                Err(ProductPromotionReadinessErrorV1::IncompleteCoverage)
            }
            _ => Err(ProductPromotionReadinessErrorV1::InvalidProbeResult),
        }
    }

    async fn run_hostile_probes(&self) -> Result<(), ProductPromotionReadinessErrorV1> {
        let mut transaction = begin_bounded_database_probe(
            &self.executor,
            &self.config.statement_timeout(),
            ScopedDatabaseProbeModeV1::SerializableReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let result = run_hostile_probes(&mut transaction).await;
        transaction.rollback().await.map_err(readiness_database)?;
        result
    }
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> ProductPromotionReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            ProductPromotionReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            ProductPromotionReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            ProductPromotionReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> ProductPromotionReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_manifest_is_exact_and_bounded() {
        assert_eq!(FUNCTIONS.len(), 8);
        assert_eq!(RELATIONS.len(), 18);
        assert_eq!(REPLAY_ARGUMENTS.matches(',').count() + 1, 22);
        assert_eq!(PREPARE_ARGUMENTS.matches(',').count() + 1, 35);
        assert_eq!(STAGE_ARGUMENTS.matches(',').count() + 1, 19);
        assert_eq!(ACTIVATION_LINK_ARGUMENTS.matches(',').count() + 1, 20);
        assert_eq!(REPAIR_LINK_ARGUMENTS.matches(',').count() + 1, 29);
        assert_eq!(KEYRING_COVERAGE_ARGUMENTS.matches(',').count() + 1, 2);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 32);
        assert_eq!(PROBE_SUBJECT_DIGEST.len(), 32);
        assert!(SUPPORT_CONTRACT_QUERY.contains("= 20"));
        assert!(SUPPORT_CONTRACT_QUERY.contains("= 19"));
        assert!(probes::HOSTILE_PROBE_QUERY.contains("LIMIT 2"));
    }

    #[test]
    fn shared_readiness_errors_keep_promotion_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            ProductPromotionReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            ProductPromotionReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            ProductPromotionReadinessErrorV1::ExcessCapability
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::Database(
                ProductDatabaseFailureV1::Timeout
            )),
            ProductPromotionReadinessErrorV1::Database(ProductDatabaseFailureV1::Timeout)
        );
    }
}
