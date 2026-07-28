use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseProbeModeV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::product_action_digest::product_action_keyring_coverage_identity_v1;
use crate::ProductDecisionReadinessErrorV1;

use super::cancellation_contract::{
    CANCEL_ARGUMENTS, CANCEL_FUNCTION, CANCEL_RESULT, DATABASE_IDENTITY_FUNCTION,
    KEYRING_COVERAGE_ARGUMENTS, KEYRING_COVERAGE_FUNCTION, KEYRING_COVERAGE_RESULT, TOPOLOGY_QUERY,
};
use super::lifecycle_cancel::PostgresProductLifecycleCancellations;
use super::readiness::{map_readiness, readiness_database, verify_approval_support_contract};

const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.lifecycle-cancellation.digest-key-fingerprint.v1";
const FUNCTIONS: [ScopedFunctionContractV1<'static>; 3] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_plpgsql_named(
        KEYRING_COVERAGE_FUNCTION,
        KEYRING_COVERAGE_RESULT,
        1.0,
        KEYRING_COVERAGE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        CANCEL_FUNCTION,
        CANCEL_RESULT,
        1.0,
        CANCEL_ARGUMENTS,
    ),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 21] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_promotions"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipts"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.product_action_receipt_idempotency_aliases",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipt_audit_evidence"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_audit_events"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_writer_fence"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_slot_writer_fences_v2"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_serving_leases"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_deployments"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_product_operations_v2"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_drain_intents_v2"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_certification_operations_v2"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.runtime_certification_operation_terminals_v2",
    ),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.runtime_product_drain_terminal_actions_v2",
    ),
];
const PROBE_SESSION_DIGEST: [u8; 32] = [47_u8; 32];
const PROBE_SUBJECT_DIGEST: [u8; 32] = [103_u8; 32];
const CANCELLATION_PROBE_QUERY: &str = "SELECT cancellation.outcome_name, \
    cancellation.exact_replay, \
    pg_catalog.jsonb_strip_nulls( \
        pg_catalog.to_jsonb(cancellation) - ARRAY['outcome_name', 'exact_replay']::TEXT[] \
    ) \
        = '{}'::JSONB AS payload_empty \
    FROM public.starring_product_cancel_runtime_drain_v2( \
        'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
        pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
        'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
        TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
        '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
        ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
        ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
        pg_catalog.repeat('7', 64), pg_catalog.repeat('8', 64), \
        pg_catalog.repeat('9', 64), 'probe reason', pg_catalog.repeat('a', 64), \
        pg_catalog.repeat('b', 32), 1, pg_catalog.repeat('c', 64), \
        pg_catalog.repeat('d', 32), 1 \
    ) AS cancellation LIMIT 2";
const CANCELLATION_SUPPORT_CONTRACT_QUERY: &str = r#"
WITH common_owner AS (
    SELECT relation.relowner AS owner_oid
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_product_drain_terminal_actions_v2'
    )
), private_functions(
    name,
    argument_identity,
    result_identity,
    volatility,
    parallel_mode,
    security_definer
) AS (
    VALUES
        (
            'starring_runtime_product_drain_cancel_root_exact_v2',
            'product_row public.runtime_product_operations_v2, drain_row public.runtime_drain_intents_v2, source_row public.runtime_deployments, requested_product_operation_id text, requested_drain_intent_id text, requested_source_intent_revision bigint, requested_source_state_digest text',
            'boolean',
            'i'::"char",
            's'::"char",
            FALSE
        ),
        (
            'starring_product_lifecycle_cancellation_record_v2',
            'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, expected_cancellation_reason text, requested_terminal_time timestamp with time zone',
            'TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text)',
            'v'::"char",
            'u'::"char",
            TRUE
        ),
        (
            'starring_product_lifecycle_cancellation_unkeyed_digest_v2',
            'requested_domain text, requested_fields text[]',
            'text',
            'i'::"char",
            's'::"char",
            FALSE
        ),
        (
            'starring_runtime_product_drain_cancelled_terminal_exact_v2',
            'action_row public.runtime_product_drain_terminal_actions_v2, product_row public.runtime_product_operations_v2, drain_row public.runtime_drain_intents_v2',
            'boolean',
            'i'::"char",
            's'::"char",
            FALSE
        ),
        (
            'starring_runtime_product_drain_cancel_source_v2',
            'requested_drain_intent_id text, requested_source_deployment_id text, requested_source_deployment_revision bigint, requested_preparation_token text, requested_binding_digest text, requested_locked_projection_digest text, requested_terminal_time timestamp with time zone',
            'public.runtime_deployments',
            'v'::"char",
            'u'::"char",
            FALSE
        )
), private_contract AS (
    SELECT pg_catalog.count(*) = 5
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND pg_catalog.pg_get_function_identity_arguments(
                function_row.oid
            ) = expected.argument_identity
            AND function_row.provolatile =
                expected.volatility
            AND function_row.proparallel =
                expected.parallel_mode
            AND function_row.prosecdef =
                expected.security_definer
            AND function_row.proisstrict
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig =
                ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_result(
                function_row.oid
            ) = expected.result_identity
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault(
                        'f',
                        function_row.proowner
                    )
                )) AS privilege
                WHERE privilege.grantee <>
                    function_row.proowner
            ),
            FALSE
        )) AS valid
    FROM private_functions AS expected
    CROSS JOIN common_owner
    CROSS JOIN LATERAL (
        SELECT namespace.oid
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspname =
            'starring_runtime_private_v2'
    ) AS private_namespace
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.pronamespace =
            private_namespace.oid
        AND function_row.proname = expected.name
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), expected_columns(name, type_name, not_null) AS (
    VALUES
        ('source_deployment_snapshot_bytes', 'bytea', FALSE),
        ('source_deployment_snapshot_digest', 'text', FALSE),
        ('source_canonical_state_bytes', 'bytea', FALSE)
), column_contract AS (
    SELECT pg_catalog.count(*) = 3
        AND pg_catalog.bool_and(COALESCE(
            attribute.attname = expected.name
            AND pg_catalog.format_type(
                attribute.atttypid,
                attribute.atttypmod
            ) = expected.type_name
            AND attribute.attnotnull = expected.not_null
            AND NOT attribute.attisdropped,
            FALSE
        )) AS valid
    FROM expected_columns AS expected
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND attribute.attname = expected.name
), constraint_contract AS (
    SELECT pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(
            constraint_row.contype = 'c'
            AND constraint_row.convalidated
            AND NOT constraint_row.condeferrable
            AND NOT constraint_row.condeferred
            AND NOT constraint_row.connoinherit
            AND constraint_row.conparentid = 0
            AND pg_catalog.strpos(
                pg_catalog.pg_get_constraintdef(
                    constraint_row.oid,
                    FALSE
                ),
                'source_deployment_snapshot_bytes'
            ) > 0
            AND pg_catalog.strpos(
                pg_catalog.pg_get_constraintdef(
                    constraint_row.oid,
                    FALSE
                ),
                'source_deployment_snapshot_digest'
            ) > 0
            AND pg_catalog.strpos(
                pg_catalog.pg_get_constraintdef(
                    constraint_row.oid,
                    FALSE
                ),
                'source_canonical_state_bytes'
            ) > 0
            AND pg_catalog.strpos(
                pg_catalog.pg_get_constraintdef(
                    constraint_row.oid,
                    FALSE
                ),
                'sha256'
            ) > 0
            AND pg_catalog.strpos(
                pg_catalog.pg_get_constraintdef(
                    constraint_row.oid,
                    FALSE
                ),
                '''consumed''::text'
            ) > 0
            AND pg_catalog.strpos(
                pg_catalog.pg_get_constraintdef(
                    constraint_row.oid,
                    FALSE
                ),
                '''cancelled''::text'
            ) > 0
        ) AS valid
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND constraint_row.conname =
            'runtime_product_drain_terminal_actions_v2_source_snapshot_check'
), expected_triggers(name, definition) AS (
    VALUES
        (
            'runtime_product_drain_terminal_actions_v2_reject_row_mutation',
            'CREATE TRIGGER runtime_product_drain_terminal_actions_v2_reject_row_mutation BEFORE DELETE OR UPDATE ON public.runtime_product_drain_terminal_actions_v2 FOR EACH ROW EXECUTE FUNCTION starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()'
        ),
        (
            'runtime_product_drain_terminal_actions_v2_reject_truncate',
            'CREATE TRIGGER runtime_product_drain_terminal_actions_v2_reject_truncate BEFORE TRUNCATE ON public.runtime_product_drain_terminal_actions_v2 FOR EACH STATEMENT EXECUTE FUNCTION starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()'
        )
), trigger_contract AS (
    SELECT pg_catalog.count(*) = 2
        AND pg_catalog.bool_and(COALESCE(
            trigger_row.oid IS NOT NULL
            AND trigger_row.tgenabled = 'O'
            AND NOT trigger_row.tgisinternal
            AND pg_catalog.pg_get_triggerdef(trigger_row.oid) =
                expected.definition
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef
            AND function_row.proconfig =
                ARRAY['search_path=pg_catalog']::TEXT[]
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault(
                        'f',
                        function_row.proowner
                    )
                )) AS privilege
                WHERE privilege.grantee <>
                    function_row.proowner
            ),
            FALSE
        )) AS valid
    FROM expected_triggers AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND trigger_row.tgname = expected.name
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = trigger_row.tgfoid
)
SELECT private_contract.valid
    AND column_contract.valid
    AND constraint_contract.valid
    AND trigger_contract.valid
FROM private_contract
CROSS JOIN column_contract
CROSS JOIN constraint_contract
CROSS JOIN trigger_contract
"#;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct CancellationProbeRow {
    outcome_name: String,
    exact_replay: bool,
    payload_empty: bool,
}

impl PostgresProductLifecycleCancellations {
    pub async fn verify_product_lifecycle_cancellation_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_product_lifecycle_cancellation_readiness()
            .await
            .map(drop)
    }

    pub(super) async fn check_product_lifecycle_cancellation_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut metadata = begin_scoped_database_readiness(
            &self.cancellation_executor,
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
        verify_scoped_schema_trust(
            &mut metadata,
            "starring_runtime_private_v2",
            DATABASE_IDENTITY_FUNCTION,
        )
        .await
        .map_err(map_readiness)?;
        verify_approval_support_contract(&mut metadata).await?;
        verify_cancellation_support_contract(&mut metadata).await?;
        let topology = load_scoped_database_topology(&mut metadata, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        if let Err(error) = self
            .check_cancellation_keyring_coverage(&mut metadata)
            .await
        {
            metadata.rollback().await.map_err(readiness_database)?;
            return Err(error);
        }
        metadata.commit().await.map_err(readiness_database)?;
        self.run_cancellation_probe().await?;
        Ok(topology)
    }

    async fn check_cancellation_keyring_coverage(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = product_action_keyring_coverage_identity_v1(
            self.config.keyring(),
            KEY_MATERIAL_FINGERPRINT_DOMAIN,
        );
        let outcomes = sqlx::query_scalar::<_, String>(
            "SELECT outcome \
             FROM public.starring_product_lifecycle_cancellation_keyring_coverage_v1($1, $2) \
             LIMIT 2",
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

    async fn run_cancellation_probe(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let mut transaction = begin_bounded_database_probe(
            &self.cancellation_executor,
            &self.config.statement_timeout(),
            ScopedDatabaseProbeModeV1::SerializableReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let result = run_cancellation_probe(&mut transaction).await;
        transaction.rollback().await.map_err(readiness_database)?;
        result
    }
}

async fn verify_cancellation_support_contract(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let valid = sqlx::query_scalar::<_, bool>(CANCELLATION_SUPPORT_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if !valid {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

async fn run_cancellation_probe(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let rows = sqlx::query_as::<_, CancellationProbeRow>(CANCELLATION_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if rows
        != [CancellationProbeRow {
            outcome_name: "invalid_input".to_string(),
            exact_replay: false,
            payload_empty: true,
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
    fn cancellation_manifest_is_exact_and_nonempty() {
        assert_eq!(FUNCTIONS.len(), 3);
        assert_eq!(RELATIONS.len(), 21);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 32);
        assert_eq!(PROBE_SUBJECT_DIGEST.len(), 32);
        assert!(CANCELLATION_SUPPORT_CONTRACT_QUERY
            .contains("pg_catalog.pg_get_function_identity_arguments("));
        assert!(CANCELLATION_SUPPORT_CONTRACT_QUERY.contains("pg_catalog.pg_get_function_result("));
        assert!(CANCELLATION_SUPPORT_CONTRACT_QUERY
            .contains("'starring_runtime_product_drain_cancel_source_v2'"));
    }
}
