use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_same_database_distinct_roles, verify_scoped_executable_allowlist,
    verify_scoped_global_user_object_deny, ScopedDatabaseProbeModeV1,
    ScopedDatabaseReadinessErrorV1, ScopedDatabaseTopologyV1, ScopedFunctionContractV1,
    ScopedRelationContractV1,
};
use crate::ProductDatabaseFailureV1;

use super::digest::keyring_coverage_identity;
use super::store::PostgresProductDecisions;

const APPROVAL_RESULT: &str = "TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text)";
const COVERAGE_RESULT: &str = "TABLE(outcome text)";
const APPROVAL_DATABASE_IDENTITY: &str =
    "public.starring_product_approval_executor_database_identity_v1()";
const APPLY_DATABASE_IDENTITY: &str =
    "public.starring_product_apply_executor_database_identity_v1()";
const APPROVAL_IDENTITY: &str = "public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)";
const COVERAGE_IDENTITY: &str =
    "public.starring_product_approval_keyring_coverage_v1(text[],text[])";
const APPROVAL_FUNCTIONS: [ScopedFunctionContractV1<'static>; 3] = [
    ScopedFunctionContractV1::scalar(APPROVAL_DATABASE_IDENTITY, "text"),
    ScopedFunctionContractV1::set_plpgsql(APPROVAL_IDENTITY, APPROVAL_RESULT, 1.0),
    ScopedFunctionContractV1::set_plpgsql(COVERAGE_IDENTITY, COVERAGE_RESULT, 1.0),
];
const APPLY_TOPOLOGY_FUNCTIONS: [ScopedFunctionContractV1<'static>; 1] =
    [ScopedFunctionContractV1::scalar(
        APPLY_DATABASE_IDENTITY,
        "text",
    )];
const APPROVAL_RELATIONS: [ScopedRelationContractV1<'static>; 16] = [
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
const TOPOLOGY_RELATIONS: [ScopedRelationContractV1<'static>; 1] =
    [ScopedRelationContractV1::ordinary_without_rls(
        "public.product_control_plane_identity",
    )];
const APPROVAL_TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_approval_executor_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const APPLY_TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_apply_executor_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
const APPROVAL_PROBE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, \
     exact_replay, guild_id \
     FROM public.starring_product_approve_v1( \
      'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
      pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
      'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
      TIMESTAMPTZ '2000-01-01T00:00:00Z', \
      TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE, 'probe_request', \
      pg_catalog.repeat('4', 64), ARRAY[pg_catalog.repeat('4', 64)], \
      ARRAY['probe_key'], ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', \
      pg_catalog.repeat('6', 64), pg_catalog.repeat('7', 64), \
      pg_catalog.repeat('8', 64))";
const PROBE_SESSION_DIGEST: [u8; 32] = [29_u8; 32];
const PROBE_SUBJECT_DIGEST: [u8; 32] = [83_u8; 32];
const APPROVAL_SUPPORT_CONTRACT_QUERY: &str = r#"
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
        ('public.activation_request_approvals',
            'public.enforce_activation_approval_payload_binding()',
            'CREATE TRIGGER activation_request_approvals_enforce_payload_binding BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_payload_binding()'),
        ('public.activation_request_approvals',
            'public.enforce_activation_approval_scope()',
            'CREATE TRIGGER activation_request_approvals_enforce_scope BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_scope()'),
        ('public.activation_request_approvals',
            'public.reject_activation_approval_mutation()',
            'CREATE TRIGGER activation_request_approvals_reject_mutation BEFORE DELETE OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.reject_activation_approval_mutation()'),
        ('public.activation_requests',
            'public.assert_atomic_product_apply_runtime_request()',
            'CREATE CONSTRAINT TRIGGER activation_requests_assert_atomic_runtime_request AFTER INSERT OR UPDATE ON public.activation_requests DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_atomic_product_apply_runtime_request()'),
        ('public.activation_requests',
            'public.assert_no_committed_product_activation_applying()',
            'CREATE CONSTRAINT TRIGGER activation_requests_assert_no_product_applying AFTER INSERT OR UPDATE ON public.activation_requests DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (((new.authority_kind = ''product_authoring''::text) AND (new.state = ''applying''::text))) EXECUTE FUNCTION public.assert_no_committed_product_activation_applying()'),
        ('public.activation_requests',
            'public.enforce_product_activation_executor()',
            'CREATE TRIGGER activation_requests_enforce_product_executor BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_executor()'),
        ('public.activation_requests',
            'public.enforce_product_activation_journal_link()',
            'CREATE TRIGGER activation_requests_enforce_product_journal_link BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_journal_link()'),
        ('public.activation_requests',
            'public.enforce_product_activation_scope()',
            'CREATE TRIGGER activation_requests_enforce_product_scope BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_scope()'),
        ('public.activation_requests',
            'public.guard_legacy_activation_product_slot()',
            'CREATE TRIGGER activation_requests_guard_legacy_product_slot BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_legacy_activation_product_slot()'),
        ('public.activation_requests',
            'public.guard_product_activation_applied_record()',
            'CREATE TRIGGER activation_requests_guard_product_applied_record BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_activation_applied_record()'),
        ('public.activation_requests',
            'public.guard_product_ruleset_artifact_transition()',
            'CREATE TRIGGER activation_requests_guard_ruleset_artifact_transition BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_ruleset_artifact_transition()'),
        ('public.product_action_receipt_audit_evidence',
            'public.reject_immutable_product_approval_row()',
            'CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_audit_evidence FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()'),
        ('public.product_action_receipt_idempotency_aliases',
            'public.enforce_product_action_receipt_alias_capacity()',
            'CREATE TRIGGER product_action_receipt_idempotency_aliases_enforce_capacity BEFORE INSERT ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_capacity()'),
        ('public.product_action_receipt_idempotency_aliases',
            'public.enforce_product_action_receipt_alias_retention()',
            'CREATE TRIGGER product_action_receipt_idempotency_aliases_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_retention()'),
        ('public.product_action_receipts',
            'public.assert_product_approval_receipt_alias()',
            'CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_alias()'),
        ('public.product_action_receipts',
            'public.assert_product_approval_receipt_audit()',
            'CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_audit AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_audit()'),
        ('public.product_action_receipts',
            'public.enforce_product_action_receipt_retention()',
            'CREATE TRIGGER product_action_receipts_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipts FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_retention()'),
        ('public.product_audit_events',
            'public.capture_product_action_receipt_audit_evidence()',
            'CREATE TRIGGER product_audit_events_capture_receipt_evidence AFTER INSERT ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence()'),
        ('public.product_audit_events',
            'public.reject_immutable_product_approval_row()',
            'CREATE TRIGGER product_audit_events_reject_mutation BEFORE DELETE OR UPDATE ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()')
), expected_triggers AS (
    SELECT pg_catalog.to_regclass(expected.relation_identity) AS relation_oid,
        pg_catalog.to_regprocedure(expected.function_identity) AS function_oid,
        expected.function_identity,
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
    SELECT (SELECT pg_catalog.count(*) FROM expected_triggers) = 19
        AND (SELECT pg_catalog.count(*) FROM actual_triggers) = 19
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
), support_functions AS (
    SELECT DISTINCT expected.function_identity
    FROM expected_triggers AS expected
), support_function_contract AS (
    SELECT pg_catalog.count(*) = 18
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proisstrict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_result(function_row.oid) = 'trigger'
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM support_functions AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), digest_helper_contract AS (
    SELECT pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(COALESCE(
            function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'i'
            AND function_row.proisstrict
            AND function_row.proparallel = 'u'
            AND NOT function_row.prosecdef
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_result(function_row.oid) = 'text'
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_desired_target_digest_v1(jsonb,bigint)'
        )
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), legacy_apply_functions(function_identity) AS (
    VALUES
        ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
        ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
        ('public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
        ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')
), legacy_apply_contract AS (
    SELECT pg_catalog.count(*) = 4
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.prosecdef
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[],
            FALSE
        )) AS valid
    FROM legacy_apply_functions AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
), schema_contract AS (
    SELECT pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(namespace.nspowner IN (
            common_owner.owner_oid,
            pg_catalog.to_regrole('pg_database_owner'),
            database_row.datdba
        ))
        AND NOT pg_catalog.bool_or(EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE privilege.privilege_type = 'CREATE'
                AND privilege.grantee <> namespace.nspowner
        )) AS valid
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN common_owner
    INNER JOIN pg_catalog.pg_database AS database_row
        ON database_row.datname = pg_catalog.current_database()
    WHERE namespace.nspname = 'public'
)
SELECT trigger_manifest.valid
    AND support_function_contract.valid
    AND digest_helper_contract.valid
    AND legacy_apply_contract.valid
    AND schema_contract.valid
FROM trigger_manifest
CROSS JOIN support_function_contract
CROSS JOIN digest_helper_contract
CROSS JOIN legacy_apply_contract
CROSS JOIN schema_contract
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDecisionReadinessErrorV1 {
    #[error("product decision database contract is invalid")]
    ContractMismatch,
    #[error("product decision database capability is missing")]
    CapabilityMissing,
    #[error("product decision database capability is excessive")]
    ExcessCapability,
    #[error("product decision keyring does not cover live receipts")]
    IncompleteCoverage,
    #[error("product decision readiness returned an invalid result")]
    InvalidResult,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApprovalProbeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
}

impl PostgresProductDecisions {
    pub async fn verify_keyring_coverage(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut transaction = begin_bounded_database_probe(
            &self.pools.approval_executor,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadOnly,
        )
        .await
        .map_err(map_readiness)?;
        let result = self.check_keyring_coverage(&mut transaction).await;
        match result {
            Ok(()) => transaction.commit().await.map_err(readiness_database),
            Err(error) => {
                transaction.rollback().await.map_err(readiness_database)?;
                Err(error)
            }
        }
    }

    pub async fn verify_approval_executor_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_approval_executor_readiness().await.map(drop)
    }

    pub async fn verify_approval_boundary_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let topologies = [
            self.check_decision_reader_readiness().await?,
            self.check_approval_executor_readiness().await?,
            check_topology(
                &self.pools.apply_executor,
                &self.config.statement_timeout(),
                &APPLY_TOPOLOGY_FUNCTIONS,
                APPLY_TOPOLOGY_QUERY,
            )
            .await?,
        ];
        verify_same_database_distinct_roles(&topologies).map_err(map_readiness)
    }

    pub async fn verify_product_decision_boundary_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let topologies = [
            self.check_decision_reader_readiness().await?,
            self.check_approval_executor_readiness().await?,
            self.check_apply_executor_readiness().await?,
        ];
        verify_same_database_distinct_roles(&topologies).map_err(map_readiness)
    }

    pub(super) async fn check_approval_executor_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut metadata = begin_scoped_database_readiness(
            &self.pools.approval_executor,
            &timeout,
            &APPROVAL_FUNCTIONS,
            &APPROVAL_RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        verify_scoped_executable_allowlist(&mut metadata, &APPROVAL_FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_global_user_object_deny(&mut metadata, &APPROVAL_FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_approval_support_contract(&mut metadata).await?;
        let topology = load_scoped_database_topology(&mut metadata, APPROVAL_TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        if let Err(error) = self.check_keyring_coverage(&mut metadata).await {
            metadata.rollback().await.map_err(readiness_database)?;
            return Err(error);
        }
        metadata.commit().await.map_err(readiness_database)?;

        let mut probe = begin_bounded_database_probe(
            &self.pools.approval_executor,
            &timeout,
            ScopedDatabaseProbeModeV1::ReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let rows = sqlx::query_as::<_, ApprovalProbeRow>(APPROVAL_PROBE_QUERY)
            .bind(PROBE_SESSION_DIGEST.as_slice())
            .bind(PROBE_SUBJECT_DIGEST.as_slice())
            .fetch_all(&mut *probe)
            .await
            .map_err(readiness_database)?;
        probe.rollback().await.map_err(readiness_database)?;
        if rows
            != [ApprovalProbeRow {
                outcome: "invalid_input".to_string(),
                resulting_revision: None,
                resulting_state: None,
                exact_replay: false,
                guild_id: None,
            }]
        {
            return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
        }
        Ok(topology)
    }

    async fn check_keyring_coverage(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = keyring_coverage_identity(self.config.keyring());
        let outcomes = sqlx::query_scalar::<_, String>(
            "SELECT outcome FROM public.starring_product_approval_keyring_coverage_v1($1, $2)",
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
}

pub(super) async fn verify_approval_support_contract(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let valid = sqlx::query_scalar::<_, bool>(APPROVAL_SUPPORT_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if !valid {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

async fn check_topology(
    pool: &sqlx::PgPool,
    timeout: &str,
    functions: &[ScopedFunctionContractV1<'_>],
    query: &str,
) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
    let mut transaction =
        begin_scoped_database_readiness(pool, timeout, functions, &TOPOLOGY_RELATIONS)
            .await
            .map_err(map_readiness)?;
    verify_scoped_executable_allowlist(&mut transaction, functions)
        .await
        .map_err(map_readiness)?;
    let topology = load_scoped_database_topology(&mut transaction, query)
        .await
        .map_err(map_readiness)?;
    transaction.commit().await.map_err(readiness_database)?;
    Ok(topology)
}

pub(super) fn map_readiness(
    error: ScopedDatabaseReadinessErrorV1,
) -> ProductDecisionReadinessErrorV1 {
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

pub(super) fn readiness_database(error: sqlx::Error) -> ProductDecisionReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_readiness_errors_keep_product_decision_classification() {
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ContractMismatch),
            ProductDecisionReadinessErrorV1::ContractMismatch
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::CapabilityMissing),
            ProductDecisionReadinessErrorV1::CapabilityMissing
        );
        assert_eq!(
            map_readiness(ScopedDatabaseReadinessErrorV1::ExcessCapability),
            ProductDecisionReadinessErrorV1::ExcessCapability
        );
    }
}
