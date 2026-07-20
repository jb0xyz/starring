pub(crate) const RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY: &str = r#"
WITH expected_columns(
    relation_identity,
    column_name,
    type_identity,
    not_null,
    default_expression
) AS (
    VALUES
        ('public.runtime_deployments', 'convergence_attempt_no', 'bigint', TRUE, '0'),
        ('public.runtime_deployments', 'last_failure_attempt_no', 'bigint', FALSE, NULL),
        ('public.runtime_attestations', 'convergence_attempt_no', 'bigint', TRUE, NULL)
), actual_columns AS (
    SELECT relation.oid AS relation_oid,
        attribute.attname::TEXT AS column_name,
        attribute.atttypid,
        attribute.atttypmod,
        attribute.attnotnull,
        attribute.attidentity,
        attribute.attgenerated,
        attribute.attcollation,
        pg_catalog.pg_get_expr(default_row.adbin, default_row.adrelid, FALSE)
            AS default_expression
    FROM pg_catalog.pg_attribute AS attribute
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = attribute.attrelid
    LEFT JOIN pg_catalog.pg_attrdef AS default_row
        ON default_row.adrelid = attribute.attrelid
        AND default_row.adnum = attribute.attnum
    WHERE relation.oid IN (
            pg_catalog.to_regclass('public.runtime_deployments'),
            pg_catalog.to_regclass('public.runtime_attestations')
        )
        AND attribute.attname IN (
            'convergence_attempt_no',
            'last_failure_attempt_no'
        )
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
), column_manifest AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_columns) = 3
        AND (SELECT pg_catalog.count(*) FROM actual_columns) = 3
        AND NOT EXISTS (
            SELECT 1
            FROM expected_columns AS expected
            FULL JOIN actual_columns AS actual
                ON actual.relation_oid = pg_catalog.to_regclass(expected.relation_identity)
                AND actual.column_name = expected.column_name
                AND actual.atttypid = pg_catalog.to_regtype(expected.type_identity)
                AND actual.atttypmod = -1
                AND actual.attnotnull = expected.not_null
                AND actual.attidentity = ''
                AND actual.attgenerated = ''
                AND actual.attcollation = 0
                AND actual.default_expression IS NOT DISTINCT FROM expected.default_expression
            WHERE expected.relation_identity IS NULL
                OR actual.relation_oid IS NULL
        ) AS valid
), expected_constraints(
    relation_identity,
    constraint_name,
    constraint_type,
    no_inherit,
    definition
) AS (
    VALUES
        ('public.runtime_deployments',
            'runtime_deployments_convergence_attempt_valid', 'c', FALSE,
            'CHECK ((((convergence_attempt_no >= 0) AND (convergence_attempt_no <= ''4294967295''::bigint)) AND ((last_failure_attempt_no IS NULL) OR ((last_failure_attempt_no >= 1) AND (last_failure_attempt_no <= convergence_attempt_no)))))'),
        ('public.runtime_attestations',
            'runtime_attestations_convergence_attempt_valid', 'c', FALSE,
            'CHECK (((convergence_attempt_no >= 1) AND (convergence_attempt_no <= ''4294967295''::bigint)))'),
        ('public.runtime_attestations',
            'runtime_attestations_deployment_attempt_unique', 'u', TRUE,
            'UNIQUE (deployment_id, convergence_attempt_no)')
), actual_constraints AS (
    SELECT constraint_row.oid AS constraint_oid,
        constraint_row.conrelid AS relation_oid,
        constraint_row.conname::TEXT AS constraint_name,
        constraint_row.contype::TEXT AS constraint_type,
        constraint_row.connoinherit,
        constraint_row.convalidated,
        constraint_row.condeferrable,
        constraint_row.condeferred,
        constraint_row.conparentid,
        pg_catalog.pg_get_constraintdef(constraint_row.oid, FALSE) AS definition
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid IN (
            pg_catalog.to_regclass('public.runtime_deployments'),
            pg_catalog.to_regclass('public.runtime_attestations')
        )
        AND constraint_row.conname IN (
            'runtime_deployments_convergence_attempt_valid',
            'runtime_attestations_convergence_attempt_valid',
            'runtime_attestations_deployment_attempt_unique'
        )
), constraint_manifest AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_constraints) = 3
        AND (SELECT pg_catalog.count(*) FROM actual_constraints) = 3
        AND NOT EXISTS (
            SELECT 1
            FROM expected_constraints AS expected
            FULL JOIN actual_constraints AS actual
                ON actual.relation_oid = pg_catalog.to_regclass(expected.relation_identity)
                AND actual.constraint_name = expected.constraint_name
                AND actual.constraint_type = expected.constraint_type
                AND actual.connoinherit = expected.no_inherit
                AND actual.convalidated
                AND NOT actual.condeferrable
                AND NOT actual.condeferred
                AND actual.conparentid = 0
                AND actual.definition = expected.definition
            WHERE expected.relation_identity IS NULL
                OR actual.constraint_oid IS NULL
        ) AS valid
), unique_index_contract AS (
    SELECT pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(COALESCE(
            index_row.indisunique
            AND index_row.indisvalid
            AND index_row.indisready
            AND index_row.indislive
            AND index_row.indimmediate
            AND NOT index_row.indisreplident
            AND index_row.indpred IS NULL
            AND index_row.indexprs IS NULL
            AND index_row.indnatts = 2
            AND index_row.indnkeyatts = 2
            AND pg_catalog.pg_get_indexdef(index_row.indexrelid)
                = 'CREATE UNIQUE INDEX runtime_attestations_deployment_attempt_unique ON public.runtime_attestations USING btree (deployment_id, convergence_attempt_no)',
            FALSE)) AS valid
    FROM pg_catalog.pg_constraint AS constraint_row
    INNER JOIN pg_catalog.pg_index AS index_row
        ON index_row.indexrelid = constraint_row.conindid
        AND index_row.indrelid = constraint_row.conrelid
    WHERE constraint_row.conrelid = pg_catalog.to_regclass('public.runtime_attestations')
        AND constraint_row.conname = 'runtime_attestations_deployment_attempt_unique'
        AND constraint_row.contype = 'u'
)
SELECT COALESCE((SELECT valid FROM column_manifest), FALSE)
    AND COALESCE((SELECT valid FROM constraint_manifest), FALSE)
    AND COALESCE((SELECT valid FROM unique_index_contract), FALSE)
"#;
