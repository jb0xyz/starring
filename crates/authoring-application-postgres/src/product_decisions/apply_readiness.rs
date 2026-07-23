use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseProbeModeV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::runtime_convergence_readiness::RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY;
use crate::ProductDecisionReadinessErrorV1;

use super::apply_contract::{
    DATABASE_IDENTITY_FUNCTION, FINALIZE_ARGUMENTS, FINALIZE_FUNCTION, FINALIZE_RESULT,
    KEYRING_COVERAGE_ARGUMENTS, KEYRING_COVERAGE_FUNCTION, KEYRING_COVERAGE_RESULT, LOCK_ARGUMENTS,
    LOCK_FUNCTION, LOCK_RESULT, TARGET_ARTIFACT_ARGUMENTS, TARGET_ARTIFACT_FUNCTION,
    TARGET_ARTIFACT_RESULT, TOPOLOGY_QUERY,
};
use super::digest::keyring_coverage_identity;
use super::readiness::{map_readiness, readiness_database, verify_approval_support_contract};
use super::store::PostgresProductDecisions;

const FUNCTIONS: [ScopedFunctionContractV1<'static>; 5] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_plpgsql_named(LOCK_FUNCTION, LOCK_RESULT, 1.0, LOCK_ARGUMENTS),
    ScopedFunctionContractV1::set_named(
        TARGET_ARTIFACT_FUNCTION,
        TARGET_ARTIFACT_RESULT,
        1.0,
        TARGET_ARTIFACT_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        FINALIZE_FUNCTION,
        FINALIZE_RESULT,
        1.0,
        FINALIZE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        KEYRING_COVERAGE_FUNCTION,
        KEYRING_COVERAGE_RESULT,
        1.0,
        KEYRING_COVERAGE_ARGUMENTS,
    ),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 19] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_request_approvals"),
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
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_activations"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_versions"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_deployments"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_writer_fence"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_serving_leases"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_attestations"),
];
const PROBE_SESSION_DIGEST: [u8; 32] = [41_u8; 32];
const PROBE_SUBJECT_DIGEST: [u8; 32] = [97_u8; 32];
const LOCK_PROBE_QUERY: &str = "SELECT outcome, exact_replay, requires_commit, \
    resulting_revision, resulting_state, deployment_id, desired_target_digest, \
    locked_projection FROM public.starring_product_apply_lock_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    'probe_receipt', 'probe_audit', 'probe_attempt', 'probe_deployment')";
const ARTIFACT_PROBE_QUERY: &str = "SELECT pg_catalog.count(*) \
    FROM public.starring_product_apply_target_artifact_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), \
    'probe_principal', $1, '1', '1')";
const FINALIZE_PROBE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, \
    exact_replay, guild_id, deployment_id, desired_target_digest \
    FROM public.starring_product_apply_finalize_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'apply', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    'probe_receipt', 'probe_audit', 'probe_attempt', 'probe_deployment', \
    '{}'::JSONB, pg_catalog.repeat('7', 64), 'null'::JSONB, '{}'::JSONB, '[]'::JSONB)";
const APPLY_SUPPORT_CONTRACT_QUERY: &str = r#"
WITH common_owner AS (
    SELECT relation.relowner AS owner_oid
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.activation_requests')
), expected_trigger_definitions(
    relation_identity,
    function_identity,
    definition
) AS (
    VALUES
        ('public.automation_ruleset_activations',
            'public.assert_product_ruleset_slot_pointer()',
            'CREATE CONSTRAINT TRIGGER automation_ruleset_activations_assert_product_slot AFTER INSERT OR DELETE OR UPDATE ON public.automation_ruleset_activations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_ruleset_slot_pointer()'),
        ('public.runtime_deployments',
            'public.enforce_runtime_deployment_policy_shadow()',
            'CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()'),
        ('public.runtime_deployments',
            'public.guard_runtime_ruleset_artifact_transition()',
            'CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()'),
        ('public.runtime_deployments',
            'public.reject_runtime_deployment_delete()',
            'CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()'),
        ('public.runtime_deployments',
            'public.validate_runtime_deployment_projection()',
            'CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()'),
        ('public.runtime_deployments',
            'public.validate_runtime_convergence_attempt_projection()',
            'CREATE TRIGGER runtime_deployments_validate_convergence_attempt BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_convergence_attempt_projection()')
), expected_triggers AS (
    SELECT pg_catalog.to_regclass(expected.relation_identity) AS relation_oid,
        pg_catalog.to_regprocedure(expected.function_identity) AS function_oid,
        expected.definition
    FROM expected_trigger_definitions AS expected
), actual_triggers AS (
    SELECT trigger_row.oid AS trigger_oid,
        trigger_row.tgrelid AS relation_oid,
        trigger_row.tgfoid AS function_oid,
        trigger_row.tgenabled::TEXT AS enabled,
        trigger_row.tgisinternal AS internal,
        trigger_row.tgparentid = 0
            AND trigger_row.tgconstrrelid = 0
            AND trigger_row.tgconstrindid = 0
            AND pg_catalog.cardinality(trigger_row.tgattr) = 0
            AND trigger_row.tgnargs = 0
            AND pg_catalog.octet_length(trigger_row.tgargs) = 0
            AND trigger_row.tgoldtable IS NULL
            AND trigger_row.tgnewtable IS NULL
            AND (
                (
                    trigger_row.tgconstraint = 0
                    AND NOT trigger_row.tgdeferrable
                    AND NOT trigger_row.tginitdeferred
                    AND constraint_row.oid IS NULL
                ) OR (
                    trigger_row.tgconstraint <> 0
                    AND constraint_row.contype = 't'
                    AND constraint_row.conname = trigger_row.tgname
                    AND constraint_row.conrelid = trigger_row.tgrelid
                    AND constraint_row.condeferrable = trigger_row.tgdeferrable
                    AND constraint_row.condeferred = trigger_row.tginitdeferred
                    AND constraint_row.convalidated
                    AND constraint_row.conparentid = 0
                )
            ) AS structural_valid,
        pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) AS definition
    FROM pg_catalog.pg_trigger AS trigger_row
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.oid = trigger_row.tgconstraint
    WHERE (
        NOT trigger_row.tgisinternal
        AND trigger_row.tgrelid IN (
            SELECT DISTINCT expected.relation_oid
            FROM expected_triggers AS expected
        )
    ) OR trigger_row.tgfoid IN (
        SELECT DISTINCT expected.function_oid
        FROM expected_triggers AS expected
    )
), trigger_manifest AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_triggers) = 6
        AND (SELECT pg_catalog.count(*) FROM actual_triggers) = 6
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
), expected_routines(
    function_identity,
    language_name,
    volatility,
    strict,
    security_definer,
    returns_set,
    rows_estimate,
    result_name
) AS (
    VALUES
        ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
            'plpgsql', 'v', TRUE, TRUE, TRUE, 1::REAL,
            'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)'),
        ('public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
            'plpgsql', 'v', TRUE, TRUE, TRUE, 1::REAL,
            'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)'),
        ('public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)',
            'plpgsql', 'v', TRUE, TRUE, FALSE, 0::REAL, 'jsonb'),
        ('public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)',
            'sql', 's', TRUE, TRUE, FALSE, 0::REAL, 'boolean'),
        ('public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'text'),
        ('public.starring_runtime_current_mutation_clock()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL,
            'timestamp with time zone'),
        ('public.assert_product_ruleset_slot_pointer()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.enforce_runtime_deployment_policy_shadow()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.guard_runtime_ruleset_artifact_transition()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.reject_runtime_deployment_delete()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.validate_runtime_deployment_projection()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.validate_runtime_convergence_attempt_projection()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger')
), routine_contract AS (
    SELECT pg_catalog.count(*) = 12
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = expected.volatility::"char"
            AND function_row.proisstrict = expected.strict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proretset = expected.returns_set
            AND function_row.prorows = expected.rows_estimate
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = expected.language_name
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_name
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM expected_routines AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
)
SELECT trigger_manifest.valid AND routine_contract.valid
FROM trigger_manifest
CROSS JOIN routine_contract
"#;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApplyLockProbeRow {
    outcome: String,
    exact_replay: bool,
    requires_commit: bool,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
    locked_projection: Option<sqlx::types::Json<serde_json::Value>>,
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApplyFinalizeProbeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
}

impl PostgresProductDecisions {
    pub async fn verify_apply_executor_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_apply_executor_readiness().await.map(drop)
    }

    pub(super) async fn check_apply_executor_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut metadata = begin_scoped_database_readiness(
            &self.pools.apply_executor,
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
        verify_apply_support_contract(&mut metadata).await?;
        let topology = load_scoped_database_topology(&mut metadata, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        if let Err(error) = self.check_apply_keyring_coverage(&mut metadata).await {
            metadata.rollback().await.map_err(readiness_database)?;
            return Err(error);
        }
        metadata.commit().await.map_err(readiness_database)?;
        self.run_apply_probes().await?;
        Ok(topology)
    }

    async fn check_apply_keyring_coverage(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = keyring_coverage_identity(self.config.keyring());
        let outcomes = sqlx::query_scalar::<_, String>(
            "SELECT outcome \
             FROM public.starring_product_apply_keyring_coverage_v1($1, $2)",
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

    async fn run_apply_probes(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let mut transaction = begin_bounded_database_probe(
            &self.pools.apply_executor,
            &self.config.statement_timeout(),
            ScopedDatabaseProbeModeV1::SerializableReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let result = run_apply_probes(&mut transaction).await;
        transaction.rollback().await.map_err(readiness_database)?;
        result
    }
}

async fn verify_apply_support_contract(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let valid = sqlx::query_scalar::<_, bool>(APPLY_SUPPORT_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    let attempt_schema_valid = sqlx::query_scalar::<_, bool>(RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if !valid || !attempt_schema_valid {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

async fn run_apply_probes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let lock_rows = sqlx::query_as::<_, ApplyLockProbeRow>(LOCK_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if !matches!(
        lock_rows.as_slice(),
        [row]
            if lock_probe_row_is_exact(row, "invalid_input")
                || lock_probe_row_is_exact(row, "runtime_writer_fenced")
    ) {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    let artifact_count = sqlx::query_scalar::<_, i64>(ARTIFACT_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if artifact_count != 0 {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    let finalize_rows = sqlx::query_as::<_, ApplyFinalizeProbeRow>(FINALIZE_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if finalize_rows
        != [ApplyFinalizeProbeRow {
            outcome: "lock_required".to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
            deployment_id: None,
            desired_target_digest: None,
        }]
    {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

fn lock_probe_row_is_exact(row: &ApplyLockProbeRow, outcome: &str) -> bool {
    row == &ApplyLockProbeRow {
        outcome: outcome.to_string(),
        exact_replay: false,
        requires_commit: false,
        resulting_revision: None,
        resulting_state: None,
        deployment_id: None,
        desired_target_digest: None,
        locked_projection: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_manifest_is_exact_and_nonempty() {
        assert_eq!(FUNCTIONS.len(), 5);
        assert_eq!(RELATIONS.len(), 19);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 32);
        assert_eq!(PROBE_SUBJECT_DIGEST.len(), 32);
    }
}
