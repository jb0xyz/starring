use crate::database_capability::{
    begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_schema_trust, ScopedDatabaseReadinessErrorV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::ProductDatabaseFailureV1;

use super::contract::{
    DATABASE_IDENTITY_FUNCTION, STATUS_ARGUMENTS, STATUS_FUNCTION, STATUS_RESULT, TOPOLOGY_QUERY,
};
use super::PostgresProductDeploymentStatuses;

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
const PROBE_SESSION_DIGEST: [u8; 31] = [113_u8; 31];
const PROBE_IDENTITY: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const SUPPORT_CONTRACT_QUERY: &str = r#"
WITH common_owner AS (
    SELECT relation.relowner AS owner_oid
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments')
), common_owner_contract AS (
    SELECT pg_catalog.count(*) = 1
        AND pg_catalog.count(owner_oid) = 1 AS valid
    FROM common_owner
), capability_function_identity_contract AS (
    SELECT pg_catalog.count(*) = 2 AS valid
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_reader_database_identity_v1',
            'starring_product_deployment_status_read_v1'
        )
), expected_support_functions(function_name, function_identity) AS (
    VALUES
        ('validate_runtime_deployment_projection',
            'public.validate_runtime_deployment_projection()'),
        ('enforce_runtime_deployment_policy_shadow',
            'public.enforce_runtime_deployment_policy_shadow()'),
        ('guard_runtime_ruleset_artifact_transition',
            'public.guard_runtime_ruleset_artifact_transition()'),
        ('reject_runtime_deployment_delete',
            'public.reject_runtime_deployment_delete()'),
        ('validate_runtime_attestation_projection',
            'public.validate_runtime_attestation_projection()'),
        ('reject_immutable_product_row',
            'public.reject_immutable_product_row()'),
        ('validate_runtime_serving_lease_transition',
            'public.validate_runtime_serving_lease_transition()'),
        ('reject_runtime_serving_lease_delete',
            'public.reject_runtime_serving_lease_delete()'),
        ('reject_ruleset_artifact_mutation',
            'public.reject_ruleset_artifact_mutation()'),
        ('starring_canonical_json_v1',
            'public.starring_canonical_json_v1(jsonb)'),
        ('starring_ruleset_content_hash_v1',
            'public.starring_ruleset_content_hash_v1(bigint,jsonb)')
), support_function_identity_contract AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_support_functions) = 11
        AND NOT EXISTS (
            SELECT 1
            FROM expected_support_functions AS expected
            WHERE pg_catalog.to_regprocedure(expected.function_identity) IS NULL
        )
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'public'
                AND function_row.proname IN (
                    SELECT expected.function_name::NAME
                    FROM expected_support_functions AS expected
                )
                AND function_row.oid NOT IN (
                    SELECT pg_catalog.to_regprocedure(expected.function_identity)
                    FROM expected_support_functions AS expected
                )
        ) AS valid
), expected_trigger_definitions(
    relation_identity,
    function_identity,
    definition,
    strict,
    security_definer,
    fixed_search_path,
    private_execution
) AS (
    VALUES
        ('public.runtime_deployments',
            'public.guard_runtime_ruleset_artifact_transition()',
            'CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.runtime_deployments',
            'public.enforce_runtime_deployment_policy_shadow()',
            'CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.runtime_deployments',
            'public.reject_runtime_deployment_delete()',
            'CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.runtime_deployments',
            'public.validate_runtime_deployment_projection()',
            'CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.runtime_attestations',
            'public.validate_runtime_attestation_projection()',
            'CREATE TRIGGER runtime_attestations_validate_projection BEFORE INSERT ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_attestation_projection()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.runtime_attestations',
            'public.reject_immutable_product_row()',
            'CREATE TRIGGER runtime_attestations_reject_mutation BEFORE DELETE OR UPDATE ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_row()',
            FALSE, FALSE, FALSE, FALSE),
        ('public.runtime_serving_leases',
            'public.validate_runtime_serving_lease_transition()',
            'CREATE TRIGGER runtime_serving_leases_validate_transition BEFORE INSERT OR UPDATE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_serving_lease_transition()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.runtime_serving_leases',
            'public.reject_runtime_serving_lease_delete()',
            'CREATE TRIGGER runtime_serving_leases_reject_delete BEFORE DELETE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_serving_lease_delete()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.automation_ruleset_versions',
            'public.reject_ruleset_artifact_mutation()',
            'CREATE TRIGGER automation_ruleset_versions_reject_mutation BEFORE DELETE OR UPDATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()',
            FALSE, TRUE, TRUE, TRUE),
        ('public.automation_ruleset_versions',
            'public.reject_ruleset_artifact_mutation()',
            'CREATE TRIGGER automation_ruleset_versions_reject_truncate BEFORE TRUNCATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()',
            FALSE, TRUE, TRUE, TRUE)
), expected_triggers AS (
    SELECT pg_catalog.to_regclass(expected.relation_identity) AS relation_oid,
        pg_catalog.to_regprocedure(expected.function_identity) AS function_oid,
        expected.function_identity,
        expected.definition,
        expected.strict,
        expected.security_definer,
        expected.fixed_search_path,
        expected.private_execution
    FROM expected_trigger_definitions AS expected
), actual_triggers AS (
    SELECT trigger_row.oid AS trigger_oid,
        trigger_row.tgrelid AS relation_oid,
        trigger_row.tgfoid AS function_oid,
        trigger_row.tgenabled::TEXT AS enabled,
        trigger_row.tgisinternal AS internal,
        trigger_row.tgparentid = 0
            AND trigger_row.tgconstraint = 0
            AND trigger_row.tgconstrrelid = 0
            AND trigger_row.tgconstrindid = 0
            AND NOT trigger_row.tgdeferrable
            AND NOT trigger_row.tginitdeferred
            AND pg_catalog.cardinality(trigger_row.tgattr) = 0
            AND trigger_row.tgnargs = 0
            AND pg_catalog.octet_length(trigger_row.tgargs) = 0
            AND trigger_row.tgoldtable IS NULL
            AND trigger_row.tgnewtable IS NULL AS structural_valid,
        pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) AS definition
    FROM pg_catalog.pg_trigger AS trigger_row
    WHERE NOT trigger_row.tgisinternal
        AND trigger_row.tgrelid IN (
            pg_catalog.to_regclass('public.runtime_deployments'),
            pg_catalog.to_regclass('public.runtime_attestations'),
            pg_catalog.to_regclass('public.runtime_serving_leases'),
            pg_catalog.to_regclass('public.automation_ruleset_versions')
        )
), trigger_manifest AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_triggers) = 10
        AND (SELECT pg_catalog.count(*) FROM actual_triggers) = 10
        AND NOT EXISTS (
            SELECT 1
            FROM expected_triggers AS expected
            FULL JOIN actual_triggers AS actual
                ON actual.relation_oid = expected.relation_oid
                AND actual.function_oid = expected.function_oid
                AND actual.definition = expected.definition
                AND actual.enabled = 'O'
                AND NOT actual.internal
                AND actual.structural_valid
            WHERE expected.relation_oid IS NULL
                OR actual.trigger_oid IS NULL
        ) AS valid
), support_function_contract AS (
    SELECT pg_catalog.count(*) = 10
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proisstrict = expected.strict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig IS NOT DISTINCT FROM CASE
                WHEN expected.fixed_search_path
                    THEN ARRAY['search_path=pg_catalog']::TEXT[]
                ELSE NULL::TEXT[]
            END
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid) = ''
            AND pg_catalog.pg_get_function_result(function_row.oid) = 'trigger'
            AND (
                NOT expected.private_execution
                OR NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        function_row.proacl,
                        pg_catalog.acldefault('f', function_row.proowner)
                    )) AS privilege
                    WHERE privilege.grantee <> function_row.proowner
                )
            ), FALSE)) AS valid
    FROM expected_triggers AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = expected.function_oid
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), digest_helper_contract AS (
    SELECT pg_catalog.count(*) = 2
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'i'
            AND function_row.proisstrict = expected.strict
            AND function_row.proparallel = expected.parallel_mode
            AND NOT function_row.prosecdef
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                = expected.identity_arguments
            AND pg_catalog.pg_get_function_result(function_row.oid) = 'text'
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM (
        VALUES
            ('public.starring_canonical_json_v1(jsonb)',
                'document jsonb', TRUE, 's'::"char"),
            ('public.starring_ruleset_content_hash_v1(bigint,jsonb)',
                'schema_version bigint, definition jsonb', TRUE, 's'::"char")
    ) AS expected(function_identity, identity_arguments, strict, parallel_mode)
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), artifact_integrity_contract AS (
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
    WHERE attribute.attrelid = pg_catalog.to_regclass(
        'public.automation_ruleset_versions'
    )
        AND attribute.attname = 'canonical_content_hash'
        AND constraint_row.conname = 'arv_content_integrity'
)
SELECT COALESCE((SELECT valid FROM trigger_manifest), FALSE)
    AND COALESCE((SELECT valid FROM common_owner_contract), FALSE)
    AND COALESCE((SELECT valid FROM capability_function_identity_contract), FALSE)
    AND COALESCE((SELECT valid FROM support_function_identity_contract), FALSE)
    AND COALESCE((SELECT valid FROM support_function_contract), FALSE)
    AND COALESCE((SELECT valid FROM digest_helper_contract), FALSE)
    AND COALESCE((SELECT valid FROM artifact_integrity_contract), FALSE)
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDeploymentStatusReadinessErrorV1 {
    #[error("product deployment-status database contract is invalid")]
    ContractMismatch,
    #[error("product deployment-status database capability is missing")]
    CapabilityMissing,
    #[error("product deployment-status database capability is excessive")]
    ExcessCapability,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

impl PostgresProductDeploymentStatuses {
    pub async fn verify_readiness(&self) -> Result<(), ProductDeploymentStatusReadinessErrorV1> {
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
        verify_scoped_schema_trust(&mut transaction, "public", DATABASE_IDENTITY_FUNCTION)
            .await
            .map_err(map_readiness)?;
        let support_valid = sqlx::query_scalar::<_, bool>(SUPPORT_CONTRACT_QUERY)
            .fetch_one(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        if !support_valid {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(ProductDeploymentStatusReadinessErrorV1::ContractMismatch);
        }
        load_scoped_database_topology(&mut transaction, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        let probe_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_deployment_status_read_v1(\
                $1, $1, $1, $1, $1, $1, $1, $1, $2)",
        )
        .bind(PROBE_IDENTITY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        if probe_count != 0 {
            transaction.rollback().await.map_err(readiness_database)?;
            return Err(ProductDeploymentStatusReadinessErrorV1::ContractMismatch);
        }
        transaction.commit().await.map_err(readiness_database)?;
        Ok(())
    }
}

fn map_readiness(error: ScopedDatabaseReadinessErrorV1) -> ProductDeploymentStatusReadinessErrorV1 {
    match error {
        ScopedDatabaseReadinessErrorV1::ContractMismatch => {
            ProductDeploymentStatusReadinessErrorV1::ContractMismatch
        }
        ScopedDatabaseReadinessErrorV1::CapabilityMissing => {
            ProductDeploymentStatusReadinessErrorV1::CapabilityMissing
        }
        ScopedDatabaseReadinessErrorV1::ExcessCapability => {
            ProductDeploymentStatusReadinessErrorV1::ExcessCapability
        }
        ScopedDatabaseReadinessErrorV1::Database(error) => error.into(),
    }
}

fn readiness_database(error: sqlx::Error) -> ProductDeploymentStatusReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_manifest_is_exact_and_nonempty() {
        assert_eq!(FUNCTIONS.len(), 2);
        assert_eq!(RELATIONS.len(), 13);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 31);
        assert!(SUPPORT_CONTRACT_QUERY.contains("pg_catalog.count(*) = 10"));
    }

    #[test]
    fn shared_readiness_errors_keep_the_status_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            ProductDeploymentStatusReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            ProductDeploymentStatusReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            ProductDeploymentStatusReadinessErrorV1::ExcessCapability
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::Database(
                ProductDatabaseFailureV1::Timeout
            )),
            ProductDeploymentStatusReadinessErrorV1::Database(ProductDatabaseFailureV1::Timeout)
        );
    }
}
