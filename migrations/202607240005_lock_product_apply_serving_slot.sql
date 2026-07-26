SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.automation_installations,
    public.runtime_deployments
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_product_apply_slot_snapshot (
    identity TEXT PRIMARY KEY,
    function_oid OID NOT NULL,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_product_apply_slot_snapshot (
    identity,
    function_oid,
    function_owner,
    function_acl
)
SELECT
    expected.identity,
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM (
    VALUES
        ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
        ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
        ('public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)')
) AS expected(identity)
INNER JOIN pg_catalog.pg_proc AS function_row
    ON function_row.oid = pg_catalog.to_regprocedure(expected.identity);

DO $preflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    external_grantee_count BIGINT;
    external_grantee OID;
    invalid_external_acl_count BIGINT;
    invalid_capability_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_writer_fence'),
            ('public.automation_installations'),
            ('public.runtime_deployments')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                '35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d'::TEXT,
                FALSE
            ),
            (
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'eb864584f7fd1d715c62ce1f1fa38b662deeaadf1b3483155ffac8fffbbff3f0'::TEXT,
                TRUE
            ),
            (
                'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                '4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b'::TEXT,
                TRUE
            )
    ) AS expected(identity, definition_digest, owner_only)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 1::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR (
            expected.owner_only
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
        );

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*) FILTER (
            WHERE privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        )
    INTO
        external_grantee_count,
        external_grantee,
        invalid_external_acl_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM (
        VALUES
            ('public.starring_product_apply_executor_database_identity_v1()'),
            ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)'),
            ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'),
            ('public.starring_product_apply_keyring_coverage_v1(text[],text[])')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> external_grantee_count
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    external_grantee_count <> 1
                    OR privilege.grantee IS DISTINCT FROM external_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_product_apply_slot_snapshot
        ) <> 3
        OR external_grantee_count > 1
        OR (external_grantee_count = 1 AND external_grantee = 0)
        OR invalid_external_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR (SELECT pg_catalog.count(*) FROM public.runtime_writer_fence) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM public.runtime_writer_fence AS fence
            WHERE fence.singleton
                AND fence.fence_state IN ('open', 'closed')
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_serving_slot_preflight_drift';
    END IF;
END;
$preflight$;

DO $replace$
DECLARE
    definition TEXT;
    previous_declaration TEXT;
    next_declaration TEXT;
    previous_delegation TEXT;
    next_delegation TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    previous_declaration :=
        'DECLARE' || E'\n' ||
        '    writer_fence_state TEXT;' || E'\n' ||
        'BEGIN';
    next_declaration :=
        'DECLARE' || E'\n' ||
        '    writer_fence_state TEXT;' || E'\n' ||
        '    serving_slot_guild_id TEXT;' || E'\n' ||
        '    serving_slot_ruleset_key TEXT;' || E'\n' ||
        'BEGIN';
    previous_delegation :=
        '    RETURN QUERY' || E'\n' ||
        '    SELECT core.*';
    next_delegation :=
        '    SELECT installation.discord_guild_id, installation.ruleset_key' || E'\n' ||
        '    INTO serving_slot_guild_id, serving_slot_ruleset_key' || E'\n' ||
        '    FROM public.automation_installations AS installation' || E'\n' ||
        '    WHERE installation.tenant_id = expected_tenant_id' || E'\n' ||
        '        AND installation.installation_id = expected_installation_id;' || E'\n' ||
        '' || E'\n' ||
        '    IF FOUND THEN' || E'\n' ||
        '        PERFORM pg_catalog.pg_advisory_xact_lock(' || E'\n' ||
        '            pg_catalog.hashtextextended(' || E'\n' ||
        '                pg_catalog.concat(' || E'\n' ||
        '                    ''starring-runtime-serving-slot-v1:'',' || E'\n' ||
        '                    serving_slot_guild_id,' || E'\n' ||
        '                    '':'',' || E'\n' ||
        '                    serving_slot_ruleset_key' || E'\n' ||
        '                ),' || E'\n' ||
        '                0' || E'\n' ||
        '            )' || E'\n' ||
        '        );' || E'\n' ||
        '' || E'\n' ||
        '        PERFORM deployment.deployment_id' || E'\n' ||
        '        FROM public.runtime_deployments AS deployment' || E'\n' ||
        '        WHERE deployment.guild_id = serving_slot_guild_id' || E'\n' ||
        '            AND deployment.ruleset_key = serving_slot_ruleset_key' || E'\n' ||
        '        ORDER BY deployment.runtime_generation, deployment.deployment_id' || E'\n' ||
        '        FOR UPDATE;' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    RETURN QUERY' || E'\n' ||
        '    SELECT core.*';

    IF pg_catalog.strpos(definition, previous_declaration) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_declaration, ''),
            previous_declaration
        ) <> 0
        OR pg_catalog.strpos(definition, previous_delegation) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_delegation, ''),
            previous_delegation
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_serving_slot_patch_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        previous_declaration,
        next_declaration
    );
    definition := pg_catalog.replace(
        definition,
        previous_delegation,
        next_delegation
    );
    EXECUTE definition;
END;
$replace$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    external_grantee_count BIGINT;
    external_grantee OID;
    invalid_external_acl_count BIGINT;
    invalid_capability_acl_count BIGINT;
    snapshot_mismatch_count BIGINT;
    wrapper_source TEXT;
    writer_position INTEGER;
    fence_position INTEGER;
    share_position INTEGER;
    invalid_position INTEGER;
    closed_position INTEGER;
    installation_position INTEGER;
    slot_position INTEGER;
    deployment_position INTEGER;
    update_position INTEGER;
    delegate_position INTEGER;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_writer_fence'),
            ('public.automation_installations'),
            ('public.runtime_deployments')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                '35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d'::TEXT,
                FALSE
            ),
            (
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'f930c836ab241c0aa56376199c0518d8cce5a446406b8503eb3f0b90ec314e38'::TEXT,
                TRUE
            ),
            (
                'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                '4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b'::TEXT,
                TRUE
            )
    ) AS expected(identity, definition_digest, owner_only)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 1::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR (
            expected.owner_only
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
        );

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*) FILTER (
            WHERE privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        )
    INTO
        external_grantee_count,
        external_grantee,
        invalid_external_acl_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM (
        VALUES
            ('public.starring_product_apply_executor_database_identity_v1()'),
            ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)'),
            ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'),
            ('public.starring_product_apply_keyring_coverage_v1(text[],text[])')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> external_grantee_count
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    external_grantee_count <> 1
                    OR privilege.grantee IS DISTINCT FROM external_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    SELECT pg_catalog.count(*)
    INTO snapshot_mismatch_count
    FROM pg_temp.starring_product_apply_slot_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner IS DISTINCT FROM snapshot.function_owner
        OR function_row.proacl IS DISTINCT FROM snapshot.function_acl;

    SELECT function_row.prosrc
    INTO wrapper_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    writer_position := pg_catalog.strpos(
        wrapper_source,
        'pg_advisory_xact_lock_shared'
    );
    fence_position := pg_catalog.strpos(
        wrapper_source,
        'FROM public.runtime_writer_fence AS fence'
    );
    share_position := pg_catalog.strpos(wrapper_source, 'FOR SHARE');
    invalid_position := pg_catalog.strpos(
        wrapper_source,
        'runtime_writer_fence_invalid'
    );
    closed_position := pg_catalog.strpos(
        wrapper_source,
        'runtime_writer_fenced'
    );
    installation_position := pg_catalog.strpos(
        wrapper_source,
        'FROM public.automation_installations AS installation'
    );
    slot_position := pg_catalog.strpos(
        wrapper_source,
        'starring-runtime-serving-slot-v1:'
    );
    deployment_position := pg_catalog.strpos(
        wrapper_source,
        'FROM public.runtime_deployments AS deployment'
    );
    update_position := pg_catalog.strpos(wrapper_source, 'FOR UPDATE');
    delegate_position := pg_catalog.strpos(
        wrapper_source,
        'starring_product_apply_lock_core_unfenced_v1'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR external_grantee_count > 1
        OR (external_grantee_count = 1 AND external_grantee = 0)
        OR invalid_external_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR snapshot_mismatch_count <> 0
        OR writer_position = 0
        OR fence_position = 0
        OR share_position = 0
        OR invalid_position = 0
        OR closed_position = 0
        OR installation_position = 0
        OR slot_position = 0
        OR deployment_position = 0
        OR update_position = 0
        OR delegate_position = 0
        OR NOT (
            writer_position < fence_position
            AND fence_position < share_position
            AND share_position < invalid_position
            AND invalid_position < closed_position
            AND closed_position < installation_position
            AND installation_position < slot_position
            AND slot_position < deployment_position
            AND deployment_position < update_position
            AND update_position < delegate_position
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_serving_slot_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
