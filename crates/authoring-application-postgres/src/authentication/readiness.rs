use chrono::{DateTime, TimeDelta, Utc};

use super::PostgresAuthentication;
use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, ScopedDatabaseProbeModeV1,
    ScopedDatabaseReadinessErrorV1, ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::ProductDatabaseFailureV1;

const READ_RESULT: &str = "TABLE(principal_id text, discord_user_id text, identity_revision bigint, display_profile jsonb, principal_disabled boolean, csrf_digest_length integer, oauth_state_digest_length integer, csrf_comparison_tag bytea, last_seen_at timestamp with time zone, idle_expires_at timestamp with time zone, absolute_expires_at timestamp with time zone, revoked_at timestamp with time zone)";
const READ_IDENTITY: &str = "public.starring_product_session_read_v1(bytea)";
const MUTATION_READ_IDENTITY: &str = "public.starring_product_session_mutation_read_v1(bytea)";
const TOUCH_IDENTITY: &str = "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)";
const FUNCTIONS: [ScopedFunctionContractV1<'static>; 3] = [
    ScopedFunctionContractV1::set(READ_IDENTITY, READ_RESULT, 1.0),
    ScopedFunctionContractV1::set(MUTATION_READ_IDENTITY, READ_RESULT, 1.0),
    ScopedFunctionContractV1::scalar(TOUCH_IDENTITY, "bigint"),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 2] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
];
const PROBE_DIGEST: [u8; 31] = [0_u8; 31];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationReadinessErrorV1 {
    #[error("authentication database contract is invalid")]
    ContractMismatch,
    #[error("authentication database capability is missing")]
    CapabilityMissing,
    #[error("authentication database capability is excessive")]
    ExcessCapability,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl PostgresAuthentication {
    pub async fn verify_readiness(&self) -> Result<(), AuthenticationReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let metadata_transaction =
            begin_scoped_database_readiness(&self.pool, &timeout, &FUNCTIONS, &RELATIONS)
                .await
                .map_err(map_readiness)?;
        metadata_transaction
            .commit()
            .await
            .map_err(readiness_database)?;

        let mut write_transaction = begin_bounded_database_probe(
            &self.pool,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let read_probe = sqlx::query_as::<_, (i64, i64)>(
            "SELECT \
             (SELECT pg_catalog.count(*) \
              FROM public.starring_product_session_read_v1($1)), \
             (SELECT pg_catalog.count(*) \
              FROM public.starring_product_session_mutation_read_v1($1))",
        )
        .bind(PROBE_DIGEST.as_slice())
        .fetch_one(&mut *write_transaction)
        .await
        .map_err(readiness_database)?;
        if read_probe != (0, 0) {
            write_transaction
                .rollback()
                .await
                .map_err(readiness_database)?;
            return Err(AuthenticationReadinessErrorV1::ContractMismatch);
        }
        let observed_last_seen_at = DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
            .expect("authentication readiness timestamp is valid");
        let observed_idle_expires_at = observed_last_seen_at + TimeDelta::minutes(5);
        let observed_absolute_expires_at = observed_idle_expires_at + TimeDelta::minutes(5);
        let touched = sqlx::query_scalar::<_, i64>(
            "SELECT public.starring_product_session_touch_v1( \
             $1, $2, $3, $4, $5)",
        )
        .bind(PROBE_DIGEST.as_slice())
        .bind(observed_last_seen_at)
        .bind(observed_idle_expires_at)
        .bind(observed_absolute_expires_at)
        .bind(60.0_f64)
        .fetch_one(&mut *write_transaction)
        .await
        .map_err(readiness_database)?;
        write_transaction
            .rollback()
            .await
            .map_err(readiness_database)?;
        if touched != 0 {
            return Err(AuthenticationReadinessErrorV1::ContractMismatch);
        }
        Ok(())
    }
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> AuthenticationReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            AuthenticationReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            AuthenticationReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            AuthenticationReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> AuthenticationReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readiness_errors_keep_the_authentication_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            AuthenticationReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            AuthenticationReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            AuthenticationReadinessErrorV1::ExcessCapability
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::Database(
                ProductDatabaseFailureV1::Timeout,
            )),
            AuthenticationReadinessErrorV1::Database(ProductDatabaseFailureV1::Timeout)
        );
    }
}
