SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.product_principals,
    public.product_auth_sessions,
    public.runtime_deployments,
    public.activation_requests,
    public.authoring_promotions,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions,
    public.runtime_attestations,
    public.runtime_serving_leases
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    relation_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    unsafe_schema_create_count BIGINT;
    function_identity_count BIGINT;
    invalid_function_count BIGINT;
    collision_count BIGINT;
    support_function_contract_valid BOOLEAN;
    attempt_contract_valid BOOLEAN;
    required_trigger_count BIGINT;
    actual_trigger_count BIGINT;
    artifact_contract_valid BOOLEAN;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF relation_count <> 13
        OR owner_count <> 1
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'product operational deployment status relations require their common owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'product operational deployment status owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_create_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.nspname = 'public'
        AND privilege.privilege_type = 'CREATE'
        AND privilege.grantee <> namespace.nspowner
        AND privilege.grantee <> pg_catalog.to_regrole('pg_database_owner');

    IF unsafe_schema_create_count <> 0
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname = 'public'
                AND namespace.nspowner IN (
                    common_owner,
                    pg_catalog.to_regrole('pg_database_owner'),
                    (
                        SELECT database_row.datdba
                        FROM pg_catalog.pg_database AS database_row
                        WHERE database_row.datname = pg_catalog.current_database()
                    )
                )
        )
    THEN
        RAISE EXCEPTION 'product operational deployment status schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_identity_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_reader_database_identity_v1',
            'starring_product_deployment_status_read_v1'
        );

    IF function_identity_count <> 2 THEN
        RAISE EXCEPTION 'legacy product deployment status function identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_deployment_status_reader_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)',
                'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone)',
                TRUE,
                1::REAL
            )
    ) AS expected(signature, identity_arguments, result_identity, returns_set, rows_estimate)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'sql'
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_identity
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee = 0
                OR (
                    privilege.grantee <> function_row.proowner
                    AND (
                        privilege.privilege_type <> 'EXECUTE'
                        OR privilege.is_grantable
                    )
                )
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'legacy product deployment status function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    WITH expected_support_functions(
        function_name,
        function_identity,
        identity_arguments,
        result_identity,
        volatility,
        strict,
        parallel_mode,
        security_definer,
        configuration,
        language_name,
        private_execution
    ) AS (
        VALUES
            ('validate_runtime_deployment_projection',
                'public.validate_runtime_deployment_projection()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('validate_runtime_convergence_attempt_projection',
                'public.validate_runtime_convergence_attempt_projection()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('enforce_runtime_deployment_policy_shadow',
                'public.enforce_runtime_deployment_policy_shadow()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('guard_runtime_ruleset_artifact_transition',
                'public.guard_runtime_ruleset_artifact_transition()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('reject_runtime_deployment_delete',
                'public.reject_runtime_deployment_delete()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('validate_runtime_attestation_projection',
                'public.validate_runtime_attestation_projection()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('validate_runtime_attestation_attempt_projection',
                'public.validate_runtime_attestation_attempt_projection()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('reject_immutable_product_row',
                'public.reject_immutable_product_row()',
                '', 'trigger', 'v', FALSE, 'u', FALSE,
                NULL::TEXT[], 'plpgsql', FALSE),
            ('validate_runtime_serving_lease_transition',
                'public.validate_runtime_serving_lease_transition()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('reject_runtime_serving_lease_delete',
                'public.reject_runtime_serving_lease_delete()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('reject_ruleset_artifact_mutation',
                'public.reject_ruleset_artifact_mutation()',
                '', 'trigger', 'v', FALSE, 'u', TRUE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('starring_canonical_json_v1',
                'public.starring_canonical_json_v1(jsonb)',
                'document jsonb', 'text', 'i', TRUE, 's', FALSE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE),
            ('starring_ruleset_content_hash_v1',
                'public.starring_ruleset_content_hash_v1(bigint,jsonb)',
                'schema_version bigint, definition jsonb', 'text', 'i', TRUE, 's', FALSE,
                ARRAY['search_path=pg_catalog']::TEXT[], 'plpgsql', TRUE)
    ), expected_contract AS (
        SELECT expected.*,
            pg_catalog.to_regprocedure(expected.function_identity) AS function_oid
        FROM expected_support_functions AS expected
    ), actual_identity_count AS (
        SELECT pg_catalog.count(*) AS function_count
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'
            AND function_row.proname IN (
                SELECT expected.function_name::NAME
                FROM expected_support_functions AS expected
            )
    )
    SELECT (SELECT pg_catalog.count(*) FROM expected_contract) = 13
        AND (SELECT function_count FROM actual_identity_count) = 13
        AND pg_catalog.bool_and(COALESCE(
            expected.function_oid IS NOT NULL
            AND function_row.proowner = common_owner
            AND function_row.prokind = 'f'
            AND function_row.provolatile::TEXT = expected.volatility
            AND function_row.proisstrict = expected.strict
            AND function_row.proparallel::TEXT = expected.parallel_mode
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND function_row.proconfig IS NOT DISTINCT FROM expected.configuration
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND language_row.lanname = expected.language_name
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                = expected.identity_arguments
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_identity
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
            ), FALSE))
    INTO support_function_contract_valid
    FROM expected_contract AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = expected.function_oid
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang;

    IF NOT COALESCE(support_function_contract_valid, FALSE) THEN
        RAISE EXCEPTION 'product operational deployment status support function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_reader_database_identity_v2',
            'starring_product_deployment_status_read_core_v2',
            'starring_product_deployment_status_read_v2'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'product operational deployment status function collides with an existing object'
            USING ERRCODE = '55000';
    END IF;

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
    ), expected_constraints(
        relation_identity,
        constraint_name,
        constraint_type,
        no_inherit,
        definition
    ) AS (
        VALUES
            (
                'public.runtime_deployments',
                'runtime_deployments_convergence_attempt_valid',
                'c',
                FALSE,
                'CHECK ((((convergence_attempt_no >= 0) AND (convergence_attempt_no <= ''4294967295''::bigint)) AND ((last_failure_attempt_no IS NULL) OR ((last_failure_attempt_no >= 1) AND (last_failure_attempt_no <= convergence_attempt_no)))))'
            ),
            (
                'public.runtime_attestations',
                'runtime_attestations_convergence_attempt_valid',
                'c',
                FALSE,
                'CHECK (((convergence_attempt_no >= 1) AND (convergence_attempt_no <= ''4294967295''::bigint)))'
            ),
            (
                'public.runtime_attestations',
                'runtime_attestations_deployment_attempt_unique',
                'u',
                TRUE,
                'UNIQUE (deployment_id, convergence_attempt_no)'
            )
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
                FALSE
            )) AS valid
        FROM pg_catalog.pg_constraint AS constraint_row
        INNER JOIN pg_catalog.pg_index AS index_row
            ON index_row.indexrelid = constraint_row.conindid
            AND index_row.indrelid = constraint_row.conrelid
        WHERE constraint_row.conrelid = pg_catalog.to_regclass('public.runtime_attestations')
            AND constraint_row.conname = 'runtime_attestations_deployment_attempt_unique'
            AND constraint_row.contype = 'u'
    )
    SELECT
        (SELECT pg_catalog.count(*) FROM actual_columns) = 3
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
        )
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
        )
        AND COALESCE((SELECT valid FROM unique_index_contract), FALSE)
    INTO attempt_contract_valid;

    IF NOT COALESCE(attempt_contract_valid, FALSE) THEN
        RAISE EXCEPTION 'runtime convergence attempt schema contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO required_trigger_count
    FROM (
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
    ) AS expected(relation_identity, function_identity, definition)
    INNER JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND trigger_row.tgfoid = pg_catalog.to_regprocedure(expected.function_identity)
        AND pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) = expected.definition
    WHERE trigger_row.tgenabled = 'O'
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
        AND trigger_row.tgnewtable IS NULL;

    SELECT pg_catalog.count(*)
    INTO actual_trigger_count
    FROM pg_catalog.pg_trigger AS trigger_row
    WHERE NOT trigger_row.tgisinternal
        AND trigger_row.tgrelid IN (
            pg_catalog.to_regclass('public.runtime_deployments'),
            pg_catalog.to_regclass('public.runtime_attestations'),
            pg_catalog.to_regclass('public.runtime_serving_leases'),
            pg_catalog.to_regclass('public.automation_ruleset_versions')
        );

    IF required_trigger_count <> 12 OR actual_trigger_count <> 12 THEN
        RAISE EXCEPTION 'product operational deployment status trigger contract is invalid'
            USING ERRCODE = '55000';
    END IF;

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
            FALSE
        ))
    INTO artifact_contract_valid
    FROM pg_catalog.pg_attribute AS attribute
    INNER JOIN pg_catalog.pg_attrdef AS attribute_default
        ON attribute_default.adrelid = attribute.attrelid
        AND attribute_default.adnum = attribute.attnum
    INNER JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = attribute.attrelid
        AND constraint_row.conname = 'arv_content_integrity'
    WHERE attribute.attrelid = pg_catalog.to_regclass('public.automation_ruleset_versions')
        AND attribute.attname = 'canonical_content_hash';

    IF NOT COALESCE(artifact_contract_valid, FALSE) THEN
        RAISE EXCEPTION 'product operational deployment status artifact contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_product_deployment_status_reader_database_identity_v2()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_deployment_status_read_core_v2(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ,
    deployment_convergence_attempt_no BIGINT,
    deployment_last_failure_attempt_no BIGINT,
    attestation_convergence_attempt_no BIGINT
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH request_clock AS MATERIALIZED (
        SELECT pg_catalog.statement_timestamp() AS database_now
    ), valid_request AS MATERIALIZED (
        SELECT request_clock.database_now
        FROM request_clock
        WHERE expected_deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND expected_promotion_id ~ '^[0-9a-f]{64}$'
            AND expected_desired_target_digest ~ '^[0-9a-f]{64}$'
            AND expected_tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND expected_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND expected_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND pg_catalog.octet_length(expected_product_session_digest) = 32
            AND CASE
                WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                    THEN expected_guild_id::NUMERIC <= 18446744073709551615
                ELSE FALSE
            END
            AND CASE
                WHEN expected_acting_discord_user_id ~ '^[1-9][0-9]{0,19}$'
                    THEN expected_acting_discord_user_id::NUMERIC
                        <= 18446744073709551615
                ELSE FALSE
            END
    ), actor_deployment AS MATERIALIZED (
        SELECT deployment.*,
            valid_request.database_now,
            deployment.promotion_id = expected_promotion_id
                AND deployment.desired_target_digest = expected_desired_target_digest
                AND deployment.guild_id = expected_guild_id AS request_matches
        FROM valid_request
        INNER JOIN public.runtime_deployments AS deployment
            ON deployment.deployment_id = expected_deployment_id
            AND deployment.tenant_id = expected_tenant_id
            AND deployment.installation_id = expected_installation_id
        INNER JOIN public.product_principals AS principal
            ON principal.principal_id = expected_principal_id
            AND principal.discord_user_id = expected_acting_discord_user_id
            AND NOT principal.disabled
        INNER JOIN public.product_auth_sessions AS product_session
            ON product_session.principal_id = principal.principal_id
            AND product_session.session_digest = expected_product_session_digest
            AND pg_catalog.octet_length(product_session.csrf_digest) = 32
            AND pg_catalog.octet_length(product_session.oauth_state_digest) = 32
            AND product_session.revoked_at IS NULL
            AND product_session.revocation_reason IS NULL
            AND product_session.authenticated_at = product_session.created_at
            AND product_session.created_at <= product_session.last_seen_at
            AND product_session.last_seen_at < product_session.idle_expires_at
            AND product_session.idle_expires_at <= product_session.absolute_expires_at
            AND product_session.idle_expires_at
                <= product_session.last_seen_at + INTERVAL '30 minutes'
            AND product_session.absolute_expires_at
                <= product_session.authenticated_at + INTERVAL '12 hours'
            AND product_session.authenticated_at <= valid_request.database_now
            AND product_session.created_at <= valid_request.database_now
            AND product_session.last_seen_at <= valid_request.database_now
            AND valid_request.database_now < product_session.idle_expires_at
            AND valid_request.database_now < product_session.absolute_expires_at
    )
    SELECT
        CASE
            WHEN actor_deployment.request_matches THEN 'exact'::TEXT
            ELSE 'request_mismatch'::TEXT
        END AS request_outcome,
        CASE WHEN actor_deployment.request_matches THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'deployment_id', actor_deployment.deployment_id,
                    'tenant_id', actor_deployment.tenant_id,
                    'installation_id', actor_deployment.installation_id,
                    'promotion_id', actor_deployment.promotion_id,
                    'activation_request_id', actor_deployment.activation_request_id,
                    'installation_authority_revision', actor_deployment.installation_authority_revision,
                    'guild_id', actor_deployment.guild_id,
                    'ruleset_key', actor_deployment.ruleset_key,
                    'target_version', actor_deployment.target_version,
                    'target_content_hash', actor_deployment.target_content_hash,
                    'binding_revision', actor_deployment.binding_revision,
                    'binding_fingerprint', actor_deployment.binding_fingerprint,
                    'desired_target_digest', actor_deployment.desired_target_digest,
                    'runtime_generation', actor_deployment.runtime_generation,
                    'previous_runtime', actor_deployment.previous_runtime,
                    'requested_at', actor_deployment.requested_at,
                    'snapshot_format_version', actor_deployment.snapshot_format_version,
                    'snapshot', actor_deployment.snapshot,
                    'revision', actor_deployment.revision,
                    'phase', actor_deployment.phase,
                    'controller_id', actor_deployment.controller_id,
                    'controller_fencing_token', actor_deployment.controller_fencing_token,
                    'controller_acquired_at', actor_deployment.controller_acquired_at,
                    'controller_lease_expires_at', actor_deployment.controller_lease_expires_at,
                    'last_fencing_token', actor_deployment.last_fencing_token,
                    'next_retry_at', actor_deployment.next_retry_at,
                    'last_stable_error_code', actor_deployment.last_stable_error_code,
                    'live_attestation_id', actor_deployment.live_attestation_id,
                    'live_at', actor_deployment.live_at,
                    'blocked_at', actor_deployment.blocked_at,
                    'superseded_at', actor_deployment.superseded_at,
                    'cancelled_at', actor_deployment.cancelled_at,
                    'created_at', actor_deployment.created_at,
                    'updated_at', actor_deployment.updated_at
                )
            )
        END AS deployment_projection,
        CASE WHEN actor_deployment.request_matches AND activation.id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'id', activation.id,
                    'tenant_id', activation.tenant_id,
                    'installation_id', activation.installation_id,
                    'guild_id', activation.guild_id,
                    'ruleset_key', activation.ruleset_key,
                    'target_version', activation.target_version,
                    'target_content_hash', activation.target_content_hash,
                    'state', activation.state,
                    'authority_kind', activation.authority_kind,
                    'link_state_name', activation.link_state_name,
                    'promotion_id', activation.promotion_id
                )
            )
        END AS activation_projection,
        CASE WHEN actor_deployment.request_matches AND promotion.id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'id', promotion.id,
                    'stage', promotion.stage,
                    'tenant_id', promotion.tenant_id,
                    'installation_id', promotion.installation_id,
                    'record_authority_tenant_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,tenant_id}'
                        ) <= 128 THEN
                            promotion.record #>> '{intent,authority,tenant_id}'
                        END,
                    'record_authority_installation_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,installation_id}'
                        ) <= 128 THEN
                            promotion.record #>> '{intent,authority,installation_id}'
                        END,
                    'record_authority_guild_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,guild_id}'
                        ) <= 20 THEN
                            promotion.record #>> '{intent,authority,guild_id}'
                        END,
                    'record_authority_ruleset_key',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,ruleset_key}'
                        ) <= 64 THEN
                            promotion.record #>> '{intent,authority,ruleset_key}'
                        END,
                    'record_authority_binding_revision',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,binding_revision}'
                        ) <= 19 THEN
                            promotion.record #>> '{intent,authority,binding_revision}'
                        END,
                    'record_context_fingerprint',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,evidence,context_fingerprint}'
                        ) <= 64 THEN
                            promotion.record #>> '{intent,evidence,context_fingerprint}'
                        END,
                    'record_activation_request_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,request_id}'
                        ) <= 64 THEN
                            promotion.record #>> '{stage,activation,request_id}'
                        END,
                    'record_activation_guild_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,guild_id}'
                        ) <= 20 THEN
                            promotion.record #>> '{stage,activation,target,guild_id}'
                        END,
                    'record_activation_ruleset_key',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,ruleset_key}'
                        ) <= 64 THEN
                            promotion.record #>> '{stage,activation,target,ruleset_key}'
                        END,
                    'record_activation_target_version',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,version}'
                        ) <= 10 THEN
                            promotion.record #>> '{stage,activation,target,version}'
                        END,
                    'record_activation_target_content_hash',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,content_hash}'
                        ) <= 64 THEN
                            promotion.record #>> '{stage,activation,target,content_hash}'
                        END
                )
            )
        END AS promotion_projection,
        CASE WHEN actor_deployment.request_matches THEN tenant.lifecycle_state END
            AS tenant_lifecycle_state,
        CASE WHEN actor_deployment.request_matches AND installation.installation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'installation_id', installation.installation_id,
                    'tenant_id', installation.tenant_id,
                    'discord_application_id', installation.discord_application_id,
                    'discord_guild_id', installation.discord_guild_id,
                    'ruleset_key', installation.ruleset_key,
                    'lifecycle_state', installation.lifecycle_state,
                    'current_authority_revision', installation.current_authority_revision
                )
            )
        END AS installation_projection,
        CASE WHEN actor_deployment.request_matches AND historical_authority.installation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'installation_id', historical_authority.installation_id,
                    'tenant_id', historical_authority.tenant_id,
                    'revision', historical_authority.revision,
                    'binding_revision', historical_authority.binding_revision,
                    'resource_bindings', historical_authority.resource_bindings,
                    'binding_fingerprint', historical_authority.binding_fingerprint
                )
            )
        END AS historical_authority_projection,
        CASE WHEN actor_deployment.request_matches AND current_authority.installation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'installation_id', current_authority.installation_id,
                    'tenant_id', current_authority.tenant_id,
                    'revision', current_authority.revision,
                    'binding_revision', current_authority.binding_revision,
                    'resource_bindings', current_authority.resource_bindings,
                    'binding_fingerprint', current_authority.binding_fingerprint,
                    'authority_payload_digest', current_authority.authority_payload_digest
                )
            )
        END AS current_authority_projection,
        CASE WHEN actor_deployment.request_matches THEN active.active_version END
            AS active_target_version,
        CASE WHEN actor_deployment.request_matches AND artifact.guild_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'schema_version', artifact.schema_version,
                    'definition', artifact.definition,
                    'content_hash', artifact.content_hash,
                    'canonical_content_hash', artifact.canonical_content_hash
                )
            )
        END AS artifact_projection,
        CASE WHEN actor_deployment.request_matches AND attestation.attestation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'attestation_id', attestation.attestation_id,
                    'attestation_digest', attestation.attestation_digest,
                    'deployment_id', attestation.deployment_id,
                    'deployment_revision', attestation.deployment_revision,
                    'tenant_id', attestation.tenant_id,
                    'installation_id', attestation.installation_id,
                    'promotion_id', attestation.promotion_id,
                    'activation_request_id', attestation.activation_request_id,
                    'guild_id', attestation.guild_id,
                    'ruleset_key', attestation.ruleset_key,
                    'target_version', attestation.target_version,
                    'target_content_hash', attestation.target_content_hash,
                    'binding_revision', attestation.binding_revision,
                    'binding_fingerprint', attestation.binding_fingerprint,
                    'runtime_generation', attestation.runtime_generation,
                    'controller_fencing_token', attestation.controller_fencing_token,
                    'process_instance_id', attestation.process_instance_id,
                    'runtime_build_revision', attestation.runtime_build_revision,
                    'panel_certificate_id', attestation.panel_certificate_id,
                    'panel_report_digest', attestation.panel_report_digest,
                    'gateway_shard_id', attestation.gateway_shard_id,
                    'gateway_ready_kind', attestation.gateway_ready_kind,
                    'gateway_ready_at', attestation.gateway_ready_at,
                    'certified_at', attestation.certified_at,
                    'record_format_version', attestation.record_format_version,
                    'record', attestation.record,
                    'created_at', attestation.created_at
                )
            )
        END AS attestation_projection,
        CASE WHEN actor_deployment.request_matches AND serving.guild_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'guild_id', serving.guild_id,
                    'ruleset_key', serving.ruleset_key,
                    'tenant_id', serving.tenant_id,
                    'installation_id', serving.installation_id,
                    'deployment_id', serving.deployment_id,
                    'attestation_id', serving.attestation_id,
                    'process_instance_id', serving.process_instance_id,
                    'runtime_generation', serving.runtime_generation,
                    'target_version', serving.target_version,
                    'target_content_hash', serving.target_content_hash,
                    'binding_revision', serving.binding_revision,
                    'binding_fingerprint', serving.binding_fingerprint,
                    'lease_epoch', serving.lease_epoch,
                    'revision', serving.revision,
                    'connected', serving.connected,
                    'serving', serving.serving,
                    'acquired_at', serving.acquired_at,
                    'last_heartbeat_at', serving.last_heartbeat_at,
                    'expires_at', serving.expires_at
                )
            )
        END AS serving_projection,
        actor_deployment.database_now,
        CASE WHEN actor_deployment.request_matches THEN
            actor_deployment.convergence_attempt_no
        END AS deployment_convergence_attempt_no,
        CASE WHEN actor_deployment.request_matches THEN
            actor_deployment.last_failure_attempt_no
        END AS deployment_last_failure_attempt_no,
        CASE WHEN actor_deployment.request_matches
            AND attestation.attestation_id IS NOT NULL THEN
            attestation.convergence_attempt_no
        END AS attestation_convergence_attempt_no
    FROM actor_deployment
    LEFT JOIN public.activation_requests AS activation
        ON actor_deployment.request_matches
        AND activation.id = actor_deployment.activation_request_id
    LEFT JOIN public.authoring_promotions AS promotion
        ON actor_deployment.request_matches
        AND promotion.id = actor_deployment.promotion_id
    LEFT JOIN public.product_tenants AS tenant
        ON actor_deployment.request_matches
        AND tenant.tenant_id = actor_deployment.tenant_id
    LEFT JOIN public.automation_installations AS installation
        ON actor_deployment.request_matches
        AND installation.tenant_id = actor_deployment.tenant_id
        AND installation.installation_id = actor_deployment.installation_id
    LEFT JOIN public.automation_installation_authority_versions AS historical_authority
        ON actor_deployment.request_matches
        AND historical_authority.tenant_id = actor_deployment.tenant_id
        AND historical_authority.installation_id = actor_deployment.installation_id
        AND historical_authority.revision
            = actor_deployment.installation_authority_revision
    LEFT JOIN public.automation_installation_authority_versions AS current_authority
        ON actor_deployment.request_matches
        AND current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id = installation.installation_id
        AND current_authority.revision = installation.current_authority_revision
    LEFT JOIN public.automation_ruleset_activations AS active
        ON actor_deployment.request_matches
        AND active.guild_id = actor_deployment.guild_id
        AND active.ruleset_key = actor_deployment.ruleset_key
    LEFT JOIN public.automation_ruleset_versions AS artifact
        ON actor_deployment.request_matches
        AND artifact.guild_id = actor_deployment.guild_id
        AND artifact.ruleset_key = actor_deployment.ruleset_key
        AND artifact.version = actor_deployment.target_version
    LEFT JOIN public.runtime_attestations AS attestation
        ON actor_deployment.request_matches
        AND actor_deployment.phase = 'live'
        AND attestation.tenant_id = actor_deployment.tenant_id
        AND attestation.installation_id = actor_deployment.installation_id
        AND attestation.deployment_id = actor_deployment.deployment_id
        AND attestation.attestation_id = actor_deployment.live_attestation_id
    LEFT JOIN public.runtime_serving_leases AS serving
        ON actor_deployment.request_matches
        AND actor_deployment.phase = 'live'
        AND serving.guild_id = actor_deployment.guild_id
        AND serving.ruleset_key = actor_deployment.ruleset_key
    LIMIT 2;
$function$;


CREATE OR REPLACE FUNCTION public.starring_product_deployment_status_read_v1(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT
        status.request_outcome,
        status.deployment_projection,
        status.activation_projection,
        status.promotion_projection,
        status.tenant_lifecycle_state,
        status.installation_projection,
        status.historical_authority_projection,
        status.current_authority_projection,
        status.active_target_version,
        status.artifact_projection,
        status.attestation_projection,
        status.serving_projection,
        status.database_now
    FROM public.starring_product_deployment_status_read_core_v2(
        expected_deployment_id => expected_deployment_id,
        expected_promotion_id => expected_promotion_id,
        expected_desired_target_digest => expected_desired_target_digest,
        expected_tenant_id => expected_tenant_id,
        expected_installation_id => expected_installation_id,
        expected_guild_id => expected_guild_id,
        expected_principal_id => expected_principal_id,
        expected_acting_discord_user_id => expected_acting_discord_user_id,
        expected_product_session_digest => expected_product_session_digest
    ) AS status
    LIMIT 2;
$function$;

CREATE FUNCTION public.starring_product_deployment_status_read_v2(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ,
    deployment_convergence_attempt_no BIGINT,
    deployment_last_failure_attempt_no BIGINT,
    attestation_convergence_attempt_no BIGINT
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT
        status.request_outcome,
        status.deployment_projection,
        status.activation_projection,
        status.promotion_projection,
        status.tenant_lifecycle_state,
        status.installation_projection,
        status.historical_authority_projection,
        status.current_authority_projection,
        status.active_target_version,
        status.artifact_projection,
        status.attestation_projection,
        status.serving_projection,
        status.database_now,
        status.deployment_convergence_attempt_no,
        status.deployment_last_failure_attempt_no,
        status.attestation_convergence_attempt_no
    FROM public.starring_product_deployment_status_read_core_v2(
        expected_deployment_id => expected_deployment_id,
        expected_promotion_id => expected_promotion_id,
        expected_desired_target_digest => expected_desired_target_digest,
        expected_tenant_id => expected_tenant_id,
        expected_installation_id => expected_installation_id,
        expected_guild_id => expected_guild_id,
        expected_principal_id => expected_principal_id,
        expected_acting_discord_user_id => expected_acting_discord_user_id,
        expected_product_session_digest => expected_product_session_digest
    ) AS status
    LIMIT 2;
$function$;

DO $postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    expected_signature TEXT;
    function_oid OID;
    grantee OID;
    grantee_name NAME;
    function_identity_count BIGINT;
    invalid_function_count BIGINT;
    probe_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'product operational deployment status owner changed during migration'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_deployment_status_reader_database_identity_v2()'),
                ('public.starring_product_deployment_status_read_core_v2(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product operational deployment status function is unavailable'
                USING ERRCODE = '55000';
        END IF;

        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            expected_signature,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            expected_signature
        );

        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'product operational deployment status grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                expected_signature,
                grantee_name
            );
        END LOOP;
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO function_identity_count
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
        );

    IF function_identity_count <> 5 THEN
        RAISE EXCEPTION 'product operational deployment status function identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_deployment_status_reader_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL,
                FALSE
            ),
            (
                'public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)',
                'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                FALSE
            ),
            (
                'public.starring_product_deployment_status_reader_database_identity_v2()',
                '',
                'text',
                FALSE,
                0::REAL,
                TRUE
            ),
            (
                'public.starring_product_deployment_status_read_core_v2(text,text,text,text,text,text,text,text,bytea)',
                'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint)',
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)',
                'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint)',
                TRUE,
                1::REAL,
                TRUE
            )
    ) AS expected(
        signature,
        identity_arguments,
        result_identity,
        returns_set,
        rows_estimate,
        owner_only
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'sql'
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_identity
        OR (
            expected.owner_only
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            )
        )
        OR (
            NOT expected.owner_only
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee = 0
                    OR (
                        privilege.grantee <> function_row.proowner
                        AND (
                            privilege.privilege_type <> 'EXECUTE'
                            OR privilege.is_grantable
                        )
                    )
            )
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product operational deployment status function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF public.starring_product_deployment_status_reader_database_identity_v2()
        IS DISTINCT FROM
        public.starring_product_deployment_status_reader_database_identity_v1()
    THEN
        RAISE EXCEPTION 'product operational deployment status database identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO probe_count
    FROM public.starring_product_deployment_status_read_v1(
        '',
        '',
        '',
        '',
        '',
        '',
        '',
        '',
        pg_catalog.decode('', 'hex')
    );

    IF probe_count <> 0 THEN
        RAISE EXCEPTION 'legacy product deployment status probe is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO probe_count
    FROM public.starring_product_deployment_status_read_v2(
        '',
        '',
        '',
        '',
        '',
        '',
        '',
        '',
        pg_catalog.decode('', 'hex')
    );

    IF probe_count <> 0 THEN
        RAISE EXCEPTION 'product operational deployment status probe is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
