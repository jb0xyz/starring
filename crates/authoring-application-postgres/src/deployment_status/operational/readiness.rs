use crate::database_capability::{
    begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseReadinessErrorV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::runtime_convergence_readiness::RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY;
use crate::ProductDatabaseFailureV1;

use super::super::readiness::SUPPORT_CONTRACT_QUERY as LEGACY_SUPPORT_CONTRACT_QUERY;
use super::contract::{
    DATABASE_IDENTITY_FUNCTION, STATUS_ARGUMENTS, STATUS_FUNCTION, STATUS_RESULT, TOPOLOGY_QUERY,
};
use super::PostgresProductDeploymentOperationalStatusesV2;

const FUNCTIONS: [ScopedFunctionContractV1<'static>; 2] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_named(STATUS_FUNCTION, STATUS_RESULT, 1.0, STATUS_ARGUMENTS),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 13] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_deployments"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_promotions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_activations"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_versions"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_attestations"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_serving_leases"),
];
const PROBE_SESSION_DIGEST: [u8; 31] = [127_u8; 31];
const PROBE_IDENTITY: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const SUPPORT_CONTRACT_QUERY: &str = r#"
WITH common_owner AS (
    SELECT relation.relowner AS owner_oid
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments')
), expected_functions(
    function_identity,
    identity_arguments,
    result_identity,
    security_definer,
    returns_set,
    rows_estimate,
    owner_only
) AS (
    VALUES
        ('public.starring_product_deployment_status_reader_database_identity_v1()',
            '', 'text', TRUE, FALSE, 0::REAL, FALSE),
        ('public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)',
            'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
            'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone)',
            TRUE, TRUE, 1::REAL, FALSE),
        ('public.starring_product_deployment_status_reader_database_identity_v2()',
            '', 'text', TRUE, FALSE, 0::REAL, FALSE),
        ('public.starring_product_deployment_status_read_core_v2(text,text,text,text,text,text,text,text,bytea)',
            'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
            'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint)',
            TRUE, TRUE, 1::REAL, TRUE),
        ('public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)',
            'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
            'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint)',
            TRUE, TRUE, 1::REAL, FALSE)
), function_contract AS (
    SELECT pg_catalog.count(*) = 5
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proisstrict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef = expected.security_definer
            AND function_row.proretset = expected.returns_set
            AND function_row.prorows = expected.rows_estimate
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'sql'
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                = expected.identity_arguments
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_identity
            AND (
                NOT expected.owner_only
                OR NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        function_row.proacl,
                        pg_catalog.acldefault('f', function_row.proowner)
                    )) AS privilege
                    WHERE privilege.grantee <> function_row.proowner
                )
            ), FALSE)) AS valid
    FROM expected_functions AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), function_identity_contract AS (
    SELECT pg_catalog.count(*) = 5 AS valid
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_reader_database_identity_v1',
            'starring_product_deployment_status_read_v1',
            'starring_product_deployment_status_reader_database_identity_v2',
            'starring_product_deployment_status_read_core_v2',
            'starring_product_deployment_status_read_v2'
        )
), expected_triggers(relation_identity, function_identity, definition) AS (
    VALUES
        ('public.runtime_deployments', 'public.guard_runtime_ruleset_artifact_transition()', 'CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()'),
        ('public.runtime_deployments', 'public.enforce_runtime_deployment_policy_shadow()', 'CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()'),
        ('public.runtime_deployments', 'public.reject_runtime_deployment_delete()', 'CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()'),
        ('public.runtime_deployments', 'public.validate_runtime_deployment_projection()', 'CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()'),
        ('public.runtime_deployments', 'public.validate_runtime_convergence_attempt_projection()', 'CREATE TRIGGER runtime_deployments_validate_convergence_attempt BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_convergence_attempt_projection()'),
        ('public.runtime_attestations', 'public.validate_runtime_attestation_projection()', 'CREATE TRIGGER runtime_attestations_validate_projection BEFORE INSERT ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_attestation_projection()'),
        ('public.runtime_attestations', 'public.validate_runtime_attestation_attempt_projection()', 'CREATE TRIGGER runtime_attestations_validate_convergence_attempt BEFORE INSERT ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_attestation_attempt_projection()'),
        ('public.runtime_attestations', 'public.reject_immutable_product_row()', 'CREATE TRIGGER runtime_attestations_reject_mutation BEFORE DELETE OR UPDATE ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_row()'),
        ('public.runtime_serving_leases', 'public.validate_runtime_serving_lease_transition()', 'CREATE TRIGGER runtime_serving_leases_validate_transition BEFORE INSERT OR UPDATE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_serving_lease_transition()'),
        ('public.runtime_serving_leases', 'public.reject_runtime_serving_lease_delete()', 'CREATE TRIGGER runtime_serving_leases_reject_delete BEFORE DELETE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_serving_lease_delete()'),
        ('public.automation_ruleset_versions', 'public.reject_ruleset_artifact_mutation()', 'CREATE TRIGGER automation_ruleset_versions_reject_mutation BEFORE DELETE OR UPDATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()'),
        ('public.automation_ruleset_versions', 'public.reject_ruleset_artifact_mutation()', 'CREATE TRIGGER automation_ruleset_versions_reject_truncate BEFORE TRUNCATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()')
), trigger_contract AS (
    SELECT pg_catalog.count(*) = 12 AS valid
    FROM expected_triggers AS expected
    INNER JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND trigger_row.tgfoid = pg_catalog.to_regprocedure(expected.function_identity)
        AND pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) = expected.definition
        AND trigger_row.tgenabled = 'O'
        AND NOT trigger_row.tgisinternal
        AND trigger_row.tgparentid = 0
        AND trigger_row.tgconstraint = 0
        AND trigger_row.tgconstrrelid = 0
        AND trigger_row.tgconstrindid = 0
        AND NOT trigger_row.tgdeferrable
        AND NOT trigger_row.tginitdeferred
        AND pg_catalog.cardinality(trigger_row.tgattr) = 0
        AND trigger_row.tgnargs = 0
        AND pg_catalog.octet_length(trigger_row.tgargs) = 0
        AND trigger_row.tgoldtable IS NULL
        AND trigger_row.tgnewtable IS NULL
), actual_trigger_contract AS (
    SELECT pg_catalog.count(*) = 12 AS valid
    FROM pg_catalog.pg_trigger AS trigger_row
    WHERE NOT trigger_row.tgisinternal
        AND trigger_row.tgrelid IN (
            pg_catalog.to_regclass('public.runtime_deployments'),
            pg_catalog.to_regclass('public.runtime_attestations'),
            pg_catalog.to_regclass('public.runtime_serving_leases'),
            pg_catalog.to_regclass('public.automation_ruleset_versions')
        )
), artifact_contract AS (
    SELECT pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(COALESCE(
            attribute.atttypid = pg_catalog.to_regtype('text')
            AND NOT attribute.attnotnull
            AND attribute.attgenerated = 's'
            AND pg_catalog.pg_get_expr(
                attribute_default.adbin,
                attribute_default.adrelid,
                FALSE
            ) = 'public.starring_ruleset_content_hash_v1(schema_version, definition)'
            AND constraint_row.contype = 'c'
            AND constraint_row.convalidated
            AND NOT constraint_row.connoinherit
            AND pg_catalog.pg_get_constraintdef(constraint_row.oid, FALSE)
                = 'CHECK (((canonical_content_hash IS NOT NULL) AND (canonical_content_hash = content_hash)))',
            FALSE)) AS valid
    FROM pg_catalog.pg_attribute AS attribute
    INNER JOIN pg_catalog.pg_attrdef AS attribute_default
        ON attribute_default.adrelid = attribute.attrelid
        AND attribute_default.adnum = attribute.attnum
    INNER JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = attribute.attrelid
    WHERE attribute.attrelid = pg_catalog.to_regclass('public.automation_ruleset_versions')
        AND attribute.attname = 'canonical_content_hash'
        AND constraint_row.conname = 'arv_content_integrity'
)
SELECT COALESCE((SELECT valid FROM function_contract), FALSE)
    AND COALESCE((SELECT valid FROM function_identity_contract), FALSE)
    AND COALESCE((SELECT valid FROM trigger_contract), FALSE)
    AND COALESCE((SELECT valid FROM actual_trigger_contract), FALSE)
    AND COALESCE((SELECT valid FROM artifact_contract), FALSE)
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDeploymentOperationalStatusReadinessErrorV2 {
    #[error("product operational deployment-status database contract is invalid")]
    ContractMismatch,
    #[error("product operational deployment-status database capability is missing")]
    CapabilityMissing,
    #[error("product operational deployment-status database capability is excessive")]
    ExcessCapability,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl PostgresProductDeploymentOperationalStatusesV2 {
    pub async fn verify_readiness(
        &self,
    ) -> Result<(), ProductDeploymentOperationalStatusReadinessErrorV2> {
        self.check_readiness().await.map(drop)
    }

    pub(crate) async fn check_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDeploymentOperationalStatusReadinessErrorV2> {
        let mut transaction = begin_scoped_database_readiness(
            &self.pool,
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
        verify_scoped_schema_trust(&mut transaction, "public", DATABASE_IDENTITY_FUNCTION)
            .await
            .map_err(map_readiness)?;
        let support_valid = sqlx::query_scalar::<_, bool>(SUPPORT_CONTRACT_QUERY)
            .fetch_one(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        let legacy_support_valid = sqlx::query_scalar::<_, bool>(LEGACY_SUPPORT_CONTRACT_QUERY)
            .fetch_one(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        let attempt_schema_valid =
            sqlx::query_scalar::<_, bool>(RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY)
                .fetch_one(&mut *transaction)
                .await
                .map_err(readiness_database)?;
        if !support_valid || !legacy_support_valid || !attempt_schema_valid {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(ProductDeploymentOperationalStatusReadinessErrorV2::ContractMismatch);
        }
        let topology = load_scoped_database_topology(&mut transaction, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        let probe_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_deployment_status_read_v2(\
                $1, $1, $1, $1, $1, $1, $1, $1, $2)",
        )
        .bind(PROBE_IDENTITY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        if probe_count != 0 {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(ProductDeploymentOperationalStatusReadinessErrorV2::ContractMismatch);
        }
        transaction.commit().await.map_err(readiness_database)?;
        Ok(topology)
    }
}

fn map_readiness(
    error: ScopedDatabaseReadinessErrorV1,
) -> ProductDeploymentOperationalStatusReadinessErrorV2 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            ProductDeploymentOperationalStatusReadinessErrorV2::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            ProductDeploymentOperationalStatusReadinessErrorV2::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            ProductDeploymentOperationalStatusReadinessErrorV2::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> ProductDeploymentOperationalStatusReadinessErrorV2 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_readiness_is_a_separate_exact_capability() {
        assert_eq!(FUNCTIONS.len(), 2);
        assert_eq!(RELATIONS.len(), 13);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 31);
        assert!(SUPPORT_CONTRACT_QUERY.contains(
            "starring_product_deployment_status_read_core_v2(text,text,text,text,text,text,text,text,bytea)"
        ));
        assert!(SUPPORT_CONTRACT_QUERY.contains("pg_catalog.count(*) = 12"));
        assert!(LEGACY_SUPPORT_CONTRACT_QUERY.contains("expected_support_functions"));
        assert!(LEGACY_SUPPORT_CONTRACT_QUERY.contains("digest_helper_contract"));
    }
}
