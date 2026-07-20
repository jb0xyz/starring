use super::manifest::{PROBE_SESSION_DIGEST, PROBE_SUBJECT_DIGEST};
use super::{readiness_database, ProductPromotionReadinessErrorV1};

pub(super) const HOSTILE_PROBE_QUERY: &str = r#"
SELECT probe_name, outcome_code, projections_empty
FROM (
    SELECT 'replay'::TEXT AS probe_name,
        result.outcome_code,
        result.promotion_record IS NULL
            AND result.admission_evidence IS NULL
            AND result.admission_digest IS NULL
            AND result.receipt_projection IS NULL
            AND result.audit_evidence_projection IS NULL
            AND result.database_now IS NOT NULL AS projections_empty
    FROM (
        SELECT * FROM public.starring_product_promotion_replay_v1(
            'probe_tenant', 'probe_installation', 'probe_principal', $1,
            '1', '1', '1', 'promote', 1, pg_catalog.repeat('2', 64),
            pg_catalog.repeat('3', 64), TIMESTAMPTZ '2000-01-01T00:00:00Z',
            TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE,
            pg_catalog.repeat('4', 64), 'probe_session', 1,
            pg_catalog.repeat('5', 64), ARRAY[pg_catalog.repeat('6', 64)],
            ARRAY['probe_key'], ARRAY[pg_catalog.repeat('7', 64)]
        ) LIMIT 2
    ) AS result
    UNION ALL
    SELECT 'prepare',
        result.outcome_code,
        result.promotion_record IS NULL
            AND result.admission_evidence IS NULL
            AND result.admission_digest IS NULL
            AND result.database_now IS NOT NULL
    FROM (
        SELECT * FROM public.starring_product_promotion_prepare_v1(
            'probe_tenant', 'probe_installation', 'probe_principal', $1,
            '1', '1', '1', 'promote', 1, pg_catalog.repeat('2', 64),
            pg_catalog.repeat('3', 64), TIMESTAMPTZ '2000-01-01T00:00:00Z',
            TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE,
            'probe_request', $2, 'probe_session', 1, 1,
            pg_catalog.repeat('4', 64), pg_catalog.repeat('5', 64),
            pg_catalog.repeat('6', 64), pg_catalog.repeat('7', 64),
            pg_catalog.jsonb_build_object('hostile', ARRAY['unexpected']),
            pg_catalog.jsonb_build_object('hostile', ARRAY['unexpected']),
            pg_catalog.repeat('8', 64), pg_catalog.repeat('9', 64),
            ARRAY[pg_catalog.repeat('9', 64)], ARRAY['probe_key'],
            ARRAY[pg_catalog.repeat('a', 64)], 'probe_key',
            pg_catalog.repeat('b', 64), pg_catalog.repeat('c', 64),
            pg_catalog.repeat('d', 64)
        ) LIMIT 2
    ) AS result
    UNION ALL
    SELECT 'publish',
        result.outcome_code,
        result.publication_projection IS NULL
            AND result.promotion_record IS NULL
            AND result.database_now IS NOT NULL
    FROM (
        SELECT * FROM public.starring_product_promotion_publish_v1(
            'probe_tenant', 'probe_installation', 'probe_principal', $1,
            '1', '1', '1', 'promote', 1, pg_catalog.repeat('2', 64),
            pg_catalog.repeat('3', 64), TIMESTAMPTZ '2000-01-01T00:00:00Z',
            TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE,
            pg_catalog.repeat('4', 64), 9223372036854775807,
            pg_catalog.repeat('5', 64), pg_catalog.repeat('6', 64)
        ) LIMIT 2
    ) AS result
    UNION ALL
    SELECT 'approval_environment',
        result.outcome_code,
        result.promotion_record IS NULL
            AND result.historical_binding_revision IS NULL
            AND result.historical_resource_bindings IS NULL
            AND result.historical_binding_fingerprint IS NULL
            AND result.active_version IS NULL
            AND result.active_content_hash IS NULL
            AND result.target_artifact_projection IS NULL
            AND result.database_now IS NOT NULL
    FROM (
        SELECT * FROM public.starring_product_promotion_approval_environment_v1(
            'probe_tenant', 'probe_installation', 'probe_principal', $1,
            '1', '1', '1', 'promote', 1, pg_catalog.repeat('2', 64),
            pg_catalog.repeat('3', 64), TIMESTAMPTZ '2000-01-01T00:00:00Z',
            TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE,
            pg_catalog.repeat('4', 64), 9223372036854775807,
            pg_catalog.repeat('5', 64), pg_catalog.repeat('6', 64)
        ) LIMIT 2
    ) AS result
    UNION ALL
    SELECT 'activation_link',
        result.outcome_code,
        result.promotion_record IS NULL
            AND result.admission_evidence IS NULL
            AND result.admission_digest IS NULL
            AND result.activation_projection IS NULL
            AND result.receipt_projection IS NULL
            AND result.audit_evidence_projection IS NULL
            AND result.database_now IS NOT NULL
    FROM (
        SELECT * FROM public.starring_product_promotion_activation_link_v1(
            'probe_tenant', 'probe_installation', 'probe_principal', $1,
            '1', '1', '1', 'promote', 1, pg_catalog.repeat('2', 64),
            pg_catalog.repeat('3', 64), TIMESTAMPTZ '2000-01-01T00:00:00Z',
            TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE,
            pg_catalog.repeat('4', 64), 2, pg_catalog.repeat('5', 64),
            pg_catalog.repeat('6', 64),
            pg_catalog.jsonb_build_object('hostile', ARRAY['unexpected'])
        ) LIMIT 2
    ) AS result
    UNION ALL
    SELECT 'repair_link',
        result.outcome_code,
        result.promotion_record IS NULL
            AND result.admission_evidence IS NULL
            AND result.admission_digest IS NULL
            AND result.activation_projection IS NULL
            AND result.receipt_projection IS NULL
            AND result.audit_evidence_projection IS NULL
            AND result.database_now IS NOT NULL
    FROM (
        SELECT * FROM public.starring_product_promotion_repair_link_v1(
            'probe_tenant', 'probe_installation', 'probe_principal', $1,
            '1', '1', '1', 'promote', 1, pg_catalog.repeat('2', 64),
            pg_catalog.repeat('3', 64), TIMESTAMPTZ '2000-01-01T00:00:00Z',
            TIMESTAMPTZ '2000-01-01T00:00:01Z', '8', TRUE,
            pg_catalog.repeat('4', 64), pg_catalog.repeat('5', 64),
            'probe_request', $2,
            pg_catalog.jsonb_build_object('hostile', ARRAY['unexpected']),
            pg_catalog.repeat('6', 64), pg_catalog.repeat('7', 64),
            ARRAY[pg_catalog.repeat('7', 64)], ARRAY['probe_key'],
            ARRAY[pg_catalog.repeat('8', 64)], 'probe_key',
            pg_catalog.repeat('9', 64), pg_catalog.repeat('a', 64),
            pg_catalog.repeat('b', 64)
        ) LIMIT 2
    ) AS result
) AS probes
ORDER BY probe_name
"#;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct HostileProbeRow {
    probe_name: String,
    outcome_code: String,
    projections_empty: bool,
}

pub(super) async fn run_hostile_probes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductPromotionReadinessErrorV1> {
    let rows = sqlx::query_as::<_, HostileProbeRow>(HOSTILE_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    let expected = [
        "activation_link",
        "approval_environment",
        "prepare",
        "publish",
        "repair_link",
        "replay",
    ];
    if rows.len() != expected.len()
        || rows.iter().zip(expected).any(|(row, name)| {
            row.probe_name != name || row.outcome_code != "access_denied" || !row.projections_empty
        })
    {
        return Err(ProductPromotionReadinessErrorV1::InvalidProbeResult);
    }
    Ok(())
}
