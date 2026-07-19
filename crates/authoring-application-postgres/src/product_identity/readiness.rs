use chrono::{DateTime, TimeDelta, Utc};

use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_same_database_distinct_roles, ScopedDatabaseProbeModeV1, ScopedDatabaseReadinessErrorV1,
    ScopedDatabaseTopologyV1, ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::ProductDatabaseFailureV1;

use super::PostgresProductIdentityStore;

const FLOW_CREATE_RESULT: &str = "TABLE(outcome_code text, redirect_uri text, return_path text, expires_at timestamp with time zone, database_now timestamp with time zone)";
const FLOW_CONSUME_RESULT: &str = "TABLE(outcome_code text, redirect_uri text, return_path text, consumed_at timestamp with time zone)";
const SESSION_ISSUE_RESULT: &str = "TABLE(outcome_code text, principal_id text, discord_user_id text, identity_revision bigint, display_profile jsonb, idle_expires_at timestamp with time zone, absolute_expires_at timestamp with time zone, database_now timestamp with time zone)";
const SESSION_READ_RESULT: &str = "TABLE(principal_id text, discord_user_id text, identity_revision bigint, display_profile jsonb, principal_disabled boolean, csrf_digest_length integer, oauth_state_digest_length integer, csrf_comparison_tag bytea, last_seen_at timestamp with time zone, idle_expires_at timestamp with time zone, absolute_expires_at timestamp with time zone, revoked_at timestamp with time zone)";
const LOGOUT_READ_RESULT: &str = "TABLE(csrf_digest_length integer, oauth_state_digest_length integer, csrf_comparison_tag bytea, last_seen_at timestamp with time zone, revoked_at timestamp with time zone, revocation_reason text)";
const SECURITY_REVOKE_RESULT: &str = "TABLE(outcome_code text)";
const OAUTH_DATABASE_IDENTITY: &str = "public.starring_product_oauth_database_identity_v1()";
const ISSUER_DATABASE_IDENTITY: &str =
    "public.starring_product_session_issuer_database_identity_v1()";
const SESSION_API_DATABASE_IDENTITY: &str =
    "public.starring_product_session_api_database_identity_v1()";
const SECURITY_DATABASE_IDENTITY: &str =
    "public.starring_product_security_revoker_database_identity_v1()";
const FLOW_CREATE_IDENTITY: &str =
    "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)";
const FLOW_CONSUME_IDENTITY: &str =
    "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])";
const SESSION_ISSUE_IDENTITY: &str = "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)";
const SESSION_READ_IDENTITY: &str = "public.starring_product_session_read_v1(bytea)";
const MUTATION_READ_IDENTITY: &str = "public.starring_product_session_mutation_read_v1(bytea)";
const SESSION_TOUCH_IDENTITY: &str = "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)";
const LOGOUT_READ_IDENTITY: &str = "public.starring_product_session_logout_read_v1(bytea)";
const LOGOUT_COMMIT_IDENTITY: &str =
    "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)";
const SECURITY_REVOKE_IDENTITY: &str = "public.starring_product_session_security_revoke_v1(bytea)";
const FLOW_FUNCTIONS: [ScopedFunctionContractV1<'static>; 3] = [
    ScopedFunctionContractV1::scalar(OAUTH_DATABASE_IDENTITY, "text"),
    ScopedFunctionContractV1::set_plpgsql(FLOW_CREATE_IDENTITY, FLOW_CREATE_RESULT, 1.0),
    ScopedFunctionContractV1::set_plpgsql(FLOW_CONSUME_IDENTITY, FLOW_CONSUME_RESULT, 1.0),
];
const ISSUER_FUNCTIONS: [ScopedFunctionContractV1<'static>; 2] = [
    ScopedFunctionContractV1::scalar(ISSUER_DATABASE_IDENTITY, "text"),
    ScopedFunctionContractV1::set_plpgsql(SESSION_ISSUE_IDENTITY, SESSION_ISSUE_RESULT, 1.0),
];
const SESSION_API_FUNCTIONS: [ScopedFunctionContractV1<'static>; 6] = [
    ScopedFunctionContractV1::scalar(SESSION_API_DATABASE_IDENTITY, "text"),
    ScopedFunctionContractV1::set(SESSION_READ_IDENTITY, SESSION_READ_RESULT, 1.0),
    ScopedFunctionContractV1::set(MUTATION_READ_IDENTITY, SESSION_READ_RESULT, 1.0),
    ScopedFunctionContractV1::scalar(SESSION_TOUCH_IDENTITY, "bigint"),
    ScopedFunctionContractV1::set(LOGOUT_READ_IDENTITY, LOGOUT_READ_RESULT, 1.0),
    ScopedFunctionContractV1::scalar(LOGOUT_COMMIT_IDENTITY, "bigint"),
];
const SECURITY_FUNCTIONS: [ScopedFunctionContractV1<'static>; 2] = [
    ScopedFunctionContractV1::scalar(SECURITY_DATABASE_IDENTITY, "text"),
    ScopedFunctionContractV1::set_plpgsql(SECURITY_REVOKE_IDENTITY, SECURITY_REVOKE_RESULT, 1.0),
];
const IDENTITY_RELATIONS: [ScopedRelationContractV1<'static>; 4] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_oauth_flows"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
];
const PROBE_DIGEST: [u8; 31] = [0_u8; 31];
const OAUTH_TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_oauth_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const ISSUER_TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_session_issuer_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const SESSION_API_TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_session_api_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const SECURITY_TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_security_revoker_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductIdentityReadinessErrorV1 {
    #[error("product identity database contract is invalid")]
    ContractMismatch,
    #[error("product identity database capability is missing")]
    CapabilityMissing,
    #[error("product identity database capability is excessive")]
    ExcessCapability,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl<G> PostgresProductIdentityStore<G> {
    pub async fn verify_readiness(&self) -> Result<(), ProductIdentityReadinessErrorV1> {
        let topologies = [
            self.check_oauth_flow_writer_readiness().await?,
            self.check_session_issuer_readiness().await?,
            self.check_session_api_readiness().await?,
            self.check_security_revoker_readiness().await?,
        ];
        verify_same_database_distinct_roles(&topologies).map_err(map_readiness)
    }

    pub async fn verify_oauth_flow_writer_readiness(
        &self,
    ) -> Result<(), ProductIdentityReadinessErrorV1> {
        self.check_oauth_flow_writer_readiness().await.map(drop)
    }

    async fn check_oauth_flow_writer_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductIdentityReadinessErrorV1> {
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let metadata = begin_scoped_database_readiness(
            &self.pools.oauth_flow_writer,
            &timeout,
            &FLOW_FUNCTIONS,
            &IDENTITY_RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        metadata.commit().await.map_err(readiness_database)?;
        let mut probe = begin_bounded_database_probe(
            &self.pools.oauth_flow_writer,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let outcomes = sqlx::query_as::<_, (String, String)>(
            "SELECT \
             (SELECT outcome_code FROM public.starring_product_oauth_flow_create_v1(\
              $1, $1, 'invalid', 'invalid', 0)), \
             (SELECT outcome_code FROM public.starring_product_oauth_flow_consume_v1(\
              $1, $1, 'invalid', ARRAY['/']))",
        )
        .bind(PROBE_DIGEST.as_slice())
        .fetch_one(&mut *probe)
        .await
        .map_err(readiness_database)?;
        let topology = load_database_topology(&mut probe, OAUTH_TOPOLOGY_QUERY).await?;
        probe.rollback().await.map_err(readiness_database)?;
        if outcomes
            != (
                "invalid_request".to_string(),
                "invalid_or_consumed".to_string(),
            )
        {
            return Err(ProductIdentityReadinessErrorV1::ContractMismatch);
        }
        Ok(topology)
    }

    pub async fn verify_session_issuer_readiness(
        &self,
    ) -> Result<(), ProductIdentityReadinessErrorV1> {
        self.check_session_issuer_readiness().await.map(drop)
    }

    async fn check_session_issuer_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductIdentityReadinessErrorV1> {
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let metadata = begin_scoped_database_readiness(
            &self.pools.session_issuer,
            &timeout,
            &ISSUER_FUNCTIONS,
            &IDENTITY_RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        metadata.commit().await.map_err(readiness_database)?;
        let mut probe = begin_bounded_database_probe(
            &self.pools.session_issuer,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let outcome = sqlx::query_scalar::<_, String>(
            "SELECT outcome_code FROM public.starring_product_session_issue_v1(\
             $1, 'invalid', '/', TIMESTAMPTZ '2000-01-01T00:00:00Z', '1', 'x', \
             $1, $1, 1, 1)",
        )
        .bind(PROBE_DIGEST.as_slice())
        .fetch_one(&mut *probe)
        .await
        .map_err(readiness_database)?;
        let topology = load_database_topology(&mut probe, ISSUER_TOPOLOGY_QUERY).await?;
        probe.rollback().await.map_err(readiness_database)?;
        if outcome != "invalid_request" {
            return Err(ProductIdentityReadinessErrorV1::ContractMismatch);
        }
        Ok(topology)
    }

    pub async fn verify_session_api_readiness(
        &self,
    ) -> Result<(), ProductIdentityReadinessErrorV1> {
        self.check_session_api_readiness().await.map(drop)
    }

    async fn check_session_api_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductIdentityReadinessErrorV1> {
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let metadata = begin_scoped_database_readiness(
            &self.pools.session_api,
            &timeout,
            &SESSION_API_FUNCTIONS,
            &IDENTITY_RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        metadata.commit().await.map_err(readiness_database)?;
        let mut probe = begin_bounded_database_probe(
            &self.pools.session_api,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let observed_last_seen_at = DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
            .expect("identity readiness timestamp is valid");
        let observed_idle_expires_at = observed_last_seen_at + TimeDelta::minutes(5);
        let observed_absolute_expires_at = observed_idle_expires_at + TimeDelta::minutes(5);
        let values = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            "SELECT \
             (SELECT pg_catalog.count(*) FROM public.starring_product_session_read_v1($1)), \
             (SELECT pg_catalog.count(*) FROM public.starring_product_session_mutation_read_v1($1)), \
             (SELECT public.starring_product_session_touch_v1($1, $2, $3, $4, 60)), \
             (SELECT pg_catalog.count(*) FROM public.starring_product_session_logout_read_v1($1)), \
             (SELECT public.starring_product_session_logout_commit_v1($1, $1, $2))",
        )
        .bind(PROBE_DIGEST.as_slice())
        .bind(observed_last_seen_at)
        .bind(observed_idle_expires_at)
        .bind(observed_absolute_expires_at)
        .fetch_one(&mut *probe)
        .await
        .map_err(readiness_database)?;
        let topology = load_database_topology(&mut probe, SESSION_API_TOPOLOGY_QUERY).await?;
        probe.rollback().await.map_err(readiness_database)?;
        if values != (0, 0, 0, 0, 0) {
            return Err(ProductIdentityReadinessErrorV1::ContractMismatch);
        }
        Ok(topology)
    }

    pub async fn verify_security_revoker_readiness(
        &self,
    ) -> Result<(), ProductIdentityReadinessErrorV1> {
        self.check_security_revoker_readiness().await.map(drop)
    }

    async fn check_security_revoker_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductIdentityReadinessErrorV1> {
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let metadata = begin_scoped_database_readiness(
            &self.pools.security_revoker,
            &timeout,
            &SECURITY_FUNCTIONS,
            &IDENTITY_RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        metadata.commit().await.map_err(readiness_database)?;
        let mut probe = begin_bounded_database_probe(
            &self.pools.security_revoker,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let outcome = sqlx::query_scalar::<_, String>(
            "SELECT outcome_code FROM public.starring_product_session_security_revoke_v1($1)",
        )
        .bind(PROBE_DIGEST.as_slice())
        .fetch_one(&mut *probe)
        .await
        .map_err(readiness_database)?;
        let topology = load_database_topology(&mut probe, SECURITY_TOPOLOGY_QUERY).await?;
        probe.rollback().await.map_err(readiness_database)?;
        if outcome != "invalid_credential" {
            return Err(ProductIdentityReadinessErrorV1::ContractMismatch);
        }
        Ok(topology)
    }
}

async fn load_database_topology(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    query: &str,
) -> Result<ScopedDatabaseTopologyV1, ProductIdentityReadinessErrorV1> {
    load_scoped_database_topology(transaction, query)
        .await
        .map_err(map_readiness)
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> ProductIdentityReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            ProductIdentityReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            ProductIdentityReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            ProductIdentityReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> ProductIdentityReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readiness_errors_keep_identity_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            ProductIdentityReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            ProductIdentityReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            ProductIdentityReadinessErrorV1::ExcessCapability
        );
    }
}
