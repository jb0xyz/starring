SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_runtime_builder_function_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_builder_function_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM pg_catalog.pg_proc AS function_row
INNER JOIN pg_catalog.pg_namespace AS namespace
    ON namespace.oid = function_row.pronamespace
WHERE namespace.nspname = 'public';

CREATE TEMPORARY TABLE pg_temp.starring_runtime_builder_capability (
    function_identity TEXT PRIMARY KEY
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_builder_capability (function_identity)
VALUES
    ('public.starring_runtime_execution_database_readiness_v1()'),
    ('public.starring_runtime_execution_database_identity_v1()'),
    ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
    ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
    ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
    ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
    ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
    ('public.starring_runtime_execution_recover_stale_live_v1()'),
    ('public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'),
    ('public.starring_runtime_gateway_owner_observe_v1(text)'),
    ('public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)'),
    ('public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)'),
    ('public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'),
    ('public.starring_runtime_writer_fence_observe_v1()'),
    ('public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)');

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
    executor_grantee OID;
    external_executor_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_manifest_acl_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.proname IN (
        'starring_runtime_json_string_bytes_v2',
        'starring_runtime_framed_digest_v2',
        'starring_runtime_product_mutation_bytes_v2',
        'starring_runtime_product_mutation_digest_v2',
        'starring_runtime_drain_intent_bytes_v2',
        'starring_runtime_drain_intent_digest_v2'
    );

    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO executor_grantee, external_executor_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM pg_temp.starring_runtime_builder_capability AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> CASE WHEN executor_grantee IS NULL THEN 1 ELSE 2 END
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee = common_owner
        ) <> 1
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE WHEN executor_grantee IS NULL THEN 0 ELSE 1 END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    privilege.grantee <> common_owner
                    AND (
                        executor_grantee IS NULL
                        OR privilege.grantee <> executor_grantee
                    )
                )
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_manifest_acl_count
    FROM (
        VALUES (
            'public.starring_runtime_execution_schema_manifest_v1()'
        )
    ) AS expected(function_identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regnamespace(
            'starring_runtime_private_v2'
        ) IS NOT NULL
        OR collision_count <> 0
        OR external_executor_count > 1
        OR executor_grantee = 0
        OR invalid_capability_acl_count <> 0
        OR invalid_manifest_acl_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '58520c287c446b96198d624c9e10c4dc7e1ba8371cc721f6192a903685197476'
        OR readiness_digest IS DISTINCT FROM
            'c819437ec90f4f64ebd8a3722979e2ea817e87bdc370eef1e5c196e163551188'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_preflight_drift';
    END IF;
END;
$preflight$;

CREATE SCHEMA starring_runtime_private_v2 AUTHORIZATION CURRENT_USER;

REVOKE ALL PRIVILEGES ON SCHEMA starring_runtime_private_v2 FROM PUBLIC;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
    input_value TEXT
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    escaped BYTEA := pg_catalog.decode('22', 'hex');
    character_value TEXT;
    codepoint INTEGER;
    character_index INTEGER;
BEGIN
    FOR character_index IN 1..pg_catalog.char_length(input_value)
    LOOP
        character_value := pg_catalog.substr(input_value, character_index, 1);
        codepoint := pg_catalog.ascii(character_value);
        CASE codepoint
            WHEN 8 THEN
                escaped := escaped || pg_catalog.decode('5c62', 'hex');
            WHEN 9 THEN
                escaped := escaped || pg_catalog.decode('5c74', 'hex');
            WHEN 10 THEN
                escaped := escaped || pg_catalog.decode('5c6e', 'hex');
            WHEN 12 THEN
                escaped := escaped || pg_catalog.decode('5c66', 'hex');
            WHEN 13 THEN
                escaped := escaped || pg_catalog.decode('5c72', 'hex');
            WHEN 34 THEN
                escaped := escaped || pg_catalog.decode('5c22', 'hex');
            WHEN 92 THEN
                escaped := escaped || pg_catalog.decode('5c5c', 'hex');
            ELSE
                IF codepoint BETWEEN 0 AND 31 THEN
                    escaped := escaped || pg_catalog.convert_to(
                        pg_catalog.concat(
                            E'\\u00',
                            pg_catalog.lpad(
                                pg_catalog.to_hex(codepoint),
                                2,
                                '0'
                            )
                        ),
                        'UTF8'
                    );
                ELSE
                    escaped := escaped || pg_catalog.convert_to(
                        character_value,
                        'UTF8'
                    );
                END IF;
        END CASE;
    END LOOP;
    RETURN escaped || pg_catalog.decode('22', 'hex');
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_framed_digest_v2(
    digest_domain BYTEA,
    canonical_payload BYTEA
)
RETURNS TEXT
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.int8send(
                pg_catalog.octet_length(digest_domain)::BIGINT
            )
            || digest_domain
            || pg_catalog.int8send(
                pg_catalog.octet_length(canonical_payload)::BIGINT
            )
            || canonical_payload
        ),
        'hex'
    )
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
    requested_operation_id TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_expected_revision BIGINT,
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT,
    requested_target_guild_id TEXT,
    requested_target_ruleset_key TEXT,
    requested_target_version BIGINT,
    requested_target_content_hash TEXT,
    requested_target_binding_revision BIGINT,
    requested_target_binding_fingerprint TEXT,
    requested_mutation_kind TEXT,
    requested_product_semantic_request_digest TEXT
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    canonical_bytes BYTEA;
BEGIN
    IF pg_catalog.octet_length(requested_operation_id) <> 32
        OR requested_operation_id !~ '^[0-9a-f]{32}$'
        OR requested_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_expected_revision NOT BETWEEN 1 AND 9223372036854775807
        OR requested_slot_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.octet_length(requested_slot_guild_id) = 20
            AND requested_slot_guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR requested_slot_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR requested_target_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.octet_length(requested_target_guild_id) = 20
            AND requested_target_guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR requested_target_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR requested_target_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.octet_length(requested_target_content_hash) <> 64
        OR requested_target_content_hash !~ '^[0-9a-f]{64}$'
        OR requested_target_binding_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.octet_length(
            requested_target_binding_fingerprint
        ) <> 64
        OR requested_target_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR requested_mutation_kind NOT IN (
            'apply',
            'supersede',
            'cancel',
            'authority_change',
            'teardown'
        )
        OR pg_catalog.octet_length(
            requested_product_semantic_request_digest
        ) <> 64
        OR requested_product_semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR requested_slot_guild_id <> requested_target_guild_id
        OR requested_slot_ruleset_key <> requested_target_ruleset_key
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_mutation_builder_input_invalid';
    END IF;

    canonical_bytes :=
        pg_catalog.convert_to(
            '{"format_version":2,"operation_id":',
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_operation_id
        )
        || pg_catalog.convert_to(',"scope":{"tenant_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_tenant_id
        )
        || pg_catalog.convert_to(',"installation_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_installation_id
        )
        || pg_catalog.convert_to(',"deployment_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_deployment_id
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                '},"expected_revision":',
                requested_expected_revision::TEXT,
                ',"slot":{"guild_id":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_slot_guild_id
        )
        || pg_catalog.convert_to(',"ruleset_key":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_slot_ruleset_key
        )
        || pg_catalog.convert_to('},"expected_target":{"guild_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_guild_id
        )
        || pg_catalog.convert_to(',"ruleset_key":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_ruleset_key
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"version":',
                requested_target_version::TEXT,
                ',"content_hash":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_content_hash
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"binding_revision":',
                requested_target_binding_revision::TEXT,
                ',"binding_fingerprint":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_binding_fingerprint
        )
        || pg_catalog.convert_to('},"mutation_kind":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_mutation_kind
        )
        || pg_catalog.convert_to(
            ',"product_semantic_request_digest":',
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_product_semantic_request_digest
        )
        || pg_catalog.convert_to('}', 'UTF8');

    IF pg_catalog.octet_length(canonical_bytes) > 32768 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_mutation_builder_output_invalid';
    END IF;

    RETURN canonical_bytes;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
    canonical_payload BYTEA
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF pg_catalog.octet_length(canonical_payload) NOT BETWEEN 1 AND 32768 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_mutation_digest_payload_invalid';
    END IF;
    RETURN starring_runtime_private_v2.starring_runtime_framed_digest_v2(
        pg_catalog.convert_to(
            'starring.runtime.product_mutation.v2',
            'UTF8'
        ) || pg_catalog.decode('00', 'hex'),
        canonical_payload
    );
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(
    requested_intent_id TEXT,
    requested_operation_id TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_expected_revision BIGINT,
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT,
    requested_target_guild_id TEXT,
    requested_target_ruleset_key TEXT,
    requested_target_version BIGINT,
    requested_target_content_hash TEXT,
    requested_target_binding_revision BIGINT,
    requested_target_binding_fingerprint TEXT,
    requested_mutation_kind TEXT,
    requested_product_semantic_request_digest TEXT
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    canonical_bytes BYTEA;
    product_bytes BYTEA;
    product_digest TEXT;
BEGIN
    IF pg_catalog.octet_length(requested_intent_id) <> 32
        OR requested_intent_id !~ '^[0-9a-f]{32}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_drain_intent_builder_input_invalid';
    END IF;

    product_bytes :=
        starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
            requested_operation_id,
            requested_tenant_id,
            requested_installation_id,
            requested_deployment_id,
            requested_expected_revision,
            requested_slot_guild_id,
            requested_slot_ruleset_key,
            requested_target_guild_id,
            requested_target_ruleset_key,
            requested_target_version,
            requested_target_content_hash,
            requested_target_binding_revision,
            requested_target_binding_fingerprint,
            requested_mutation_kind,
            requested_product_semantic_request_digest
        );
    product_digest :=
        starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
            product_bytes
        );

    canonical_bytes :=
        pg_catalog.convert_to(
            '{"format_version":2,"key":{"intent_id":',
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_intent_id
        )
        || pg_catalog.convert_to(',"product_operation_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_operation_id
        )
        || pg_catalog.convert_to(',"product_mutation_digest":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            product_digest
        )
        || pg_catalog.convert_to(',"scope":{"tenant_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_tenant_id
        )
        || pg_catalog.convert_to(',"installation_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_installation_id
        )
        || pg_catalog.convert_to(',"deployment_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_deployment_id
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                '},"expected_revision":',
                requested_expected_revision::TEXT,
                ',"slot":{"guild_id":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_slot_guild_id
        )
        || pg_catalog.convert_to(',"ruleset_key":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_slot_ruleset_key
        )
        || pg_catalog.convert_to('},"expected_target":{"guild_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_guild_id
        )
        || pg_catalog.convert_to(',"ruleset_key":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_ruleset_key
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"version":',
                requested_target_version::TEXT,
                ',"content_hash":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_content_hash
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"binding_revision":',
                requested_target_binding_revision::TEXT,
                ',"binding_fingerprint":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_binding_fingerprint
        )
        || pg_catalog.convert_to('},"mutation_kind":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_mutation_kind
        )
        || pg_catalog.convert_to('}}', 'UTF8');

    IF pg_catalog.octet_length(canonical_bytes) > 65536 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_drain_intent_builder_output_invalid';
    END IF;

    RETURN canonical_bytes;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(
    canonical_payload BYTEA
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF pg_catalog.octet_length(canonical_payload) NOT BETWEEN 1 AND 65536 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_drain_intent_digest_payload_invalid';
    END IF;
    RETURN starring_runtime_private_v2.starring_runtime_framed_digest_v2(
        pg_catalog.convert_to(
            'starring.runtime.drain_intent.v2',
            'UTF8'
        ) || pg_catalog.decode('00', 'hex'),
        canonical_payload
    );
END;
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(TEXT),
    starring_runtime_private_v2.starring_runtime_framed_digest_v2(BYTEA,BYTEA),
    starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
        TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,
        TEXT,TEXT,TEXT
    ),
    starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(
        TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,
        BIGINT,TEXT,TEXT,TEXT
    ),
    starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(BYTEA)
FROM PUBLIC;

DO $patch_manifest$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    previous_fragment := '    WITH protected(relation_oid) AS (';
    next_fragment :=
        '    WITH protected_schema(namespace_oid) AS (' || E'\n' ||
        '        VALUES' || E'\n' ||
        '            (pg_catalog.to_regnamespace(''starring_runtime_private_v2''))' || E'\n' ||
        '    ), protected(relation_oid) AS (';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_manifest_schema_cte_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_framed_digest_v2(bytea,bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_manifest_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    ), manifest(value) AS (' || E'\n' ||
        '        SELECT pg_catalog.concat_ws(' || E'\n' ||
        '            ''|'',' || E'\n' ||
        '            ''relation'',';
    next_fragment :=
        '    ), manifest(value) AS (' || E'\n' ||
        '        SELECT pg_catalog.concat_ws(' || E'\n' ||
        '            ''|'',' || E'\n' ||
        '            ''schema'',' || E'\n' ||
        '            namespace.nspname,' || E'\n' ||
        '            (namespace.nspowner = (' || E'\n' ||
        '                SELECT relation.relowner' || E'\n' ||
        '                FROM pg_catalog.pg_class AS relation' || E'\n' ||
        '                WHERE relation.oid = pg_catalog.to_regclass(''public.runtime_deployments'')' || E'\n' ||
        '            ))::TEXT,' || E'\n' ||
        '            ((SELECT pg_catalog.count(*)' || E'\n' ||
        '                FROM pg_catalog.aclexplode(COALESCE(' || E'\n' ||
        '                    namespace.nspacl,' || E'\n' ||
        '                    pg_catalog.acldefault(''n'', namespace.nspowner)' || E'\n' ||
        '                )) AS privilege' || E'\n' ||
        '            ) = 2)::TEXT,' || E'\n' ||
        '            (NOT EXISTS (' || E'\n' ||
        '                SELECT 1' || E'\n' ||
        '                FROM pg_catalog.aclexplode(COALESCE(' || E'\n' ||
        '                    namespace.nspacl,' || E'\n' ||
        '                    pg_catalog.acldefault(''n'', namespace.nspowner)' || E'\n' ||
        '                )) AS privilege' || E'\n' ||
        '                WHERE privilege.grantee <> namespace.nspowner' || E'\n' ||
        '                    OR privilege.grantor <> namespace.nspowner' || E'\n' ||
        '                    OR privilege.privilege_type NOT IN (''USAGE'', ''CREATE'')' || E'\n' ||
        '                    OR privilege.is_grantable' || E'\n' ||
        '            ))::TEXT' || E'\n' ||
        '        )' || E'\n' ||
        '        FROM pg_catalog.pg_namespace AS namespace' || E'\n' ||
        '        WHERE namespace.oid IN (' || E'\n' ||
        '            SELECT namespace_oid' || E'\n' ||
        '            FROM protected_schema' || E'\n' ||
        '            WHERE namespace_oid IS NOT NULL' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION ALL' || E'\n' ||
        '        SELECT pg_catalog.concat_ws(' || E'\n' ||
        '            ''|'',' || E'\n' ||
        '            ''relation'',';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_manifest_schema_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RETURN observed_count = 574' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''849f36c9bd2d04e19008a3917aff07ede45fdda06f5bd1824b8e9c622077bc24'';';
    next_fragment :=
        '    RETURN observed_count = 581' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''944f87185d6fd290c3b9a2fe2de08ec097c833802292a2ed34c80c811c5ee062'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_manifest_expectation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_manifest$;

DO $patch_readiness$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    previous_fragment :=
        '    invalid_protected_function_count BIGINT;' || E'\n' ||
        '    identity_count BIGINT;';
    next_fragment :=
        '    invalid_protected_function_count BIGINT;' || E'\n' ||
        '    invalid_private_helper_acl_count BIGINT;' || E'\n' ||
        '    invalid_private_schema_count BIGINT;' || E'\n' ||
        '    identity_count BIGINT;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_readiness_declare_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''public.reject_runtime_product_drain_mutation()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    next_fragment :=
        '            (''public.reject_runtime_product_drain_mutation()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_framed_digest_v2(bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_readiness_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    SELECT pg_catalog.count(*)' || E'\n' ||
        '    INTO unsafe_schema_count' || E'\n' ||
        '    FROM pg_catalog.pg_namespace AS namespace';
    next_fragment :=
        '    SELECT pg_catalog.count(*)' || E'\n' ||
        '    INTO invalid_private_helper_acl_count' || E'\n' ||
        '    FROM (' || E'\n' ||
        '        VALUES' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_framed_digest_v2(bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)'')' || E'\n' ||
        '    ) AS expected(identity)' || E'\n' ||
        '    LEFT JOIN pg_catalog.pg_proc AS function_row' || E'\n' ||
        '        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)' || E'\n' ||
        '    WHERE function_row.oid IS NULL' || E'\n' ||
        '        OR (' || E'\n' ||
        '            SELECT pg_catalog.count(*)' || E'\n' ||
        '            FROM pg_catalog.aclexplode(COALESCE(' || E'\n' ||
        '                function_row.proacl,' || E'\n' ||
        '                pg_catalog.acldefault(''f'', function_row.proowner)' || E'\n' ||
        '            )) AS privilege' || E'\n' ||
        '        ) <> 1' || E'\n' ||
        '        OR EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM pg_catalog.aclexplode(COALESCE(' || E'\n' ||
        '                function_row.proacl,' || E'\n' ||
        '                pg_catalog.acldefault(''f'', function_row.proowner)' || E'\n' ||
        '            )) AS privilege' || E'\n' ||
        '            WHERE privilege.grantee <> common_owner' || E'\n' ||
        '                OR privilege.grantor <> common_owner' || E'\n' ||
        '                OR privilege.privilege_type <> ''EXECUTE''' || E'\n' ||
        '                OR privilege.is_grantable' || E'\n' ||
        '        );' || E'\n' ||
        '' || E'\n' ||
        '    IF invalid_private_helper_acl_count <> 0 THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RE001'',' || E'\n' ||
        '            MESSAGE = ''runtime_execution_database_private_helper_acl_drift'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT pg_catalog.count(*)' || E'\n' ||
        '    INTO invalid_private_schema_count' || E'\n' ||
        '    FROM (' || E'\n' ||
        '        VALUES (''starring_runtime_private_v2'')' || E'\n' ||
        '    ) AS expected(schema_name)' || E'\n' ||
        '    LEFT JOIN pg_catalog.pg_namespace AS namespace' || E'\n' ||
        '        ON namespace.nspname = expected.schema_name' || E'\n' ||
        '    WHERE namespace.oid IS NULL' || E'\n' ||
        '        OR namespace.nspowner <> common_owner' || E'\n' ||
        '        OR pg_catalog.has_schema_privilege(' || E'\n' ||
        '            invoker_oid,' || E'\n' ||
        '            namespace.oid,' || E'\n' ||
        '            ''USAGE''' || E'\n' ||
        '        )' || E'\n' ||
        '        OR pg_catalog.has_schema_privilege(' || E'\n' ||
        '            invoker_oid,' || E'\n' ||
        '            namespace.oid,' || E'\n' ||
        '            ''CREATE''' || E'\n' ||
        '        )' || E'\n' ||
        '        OR (' || E'\n' ||
        '            SELECT pg_catalog.count(*)' || E'\n' ||
        '            FROM pg_catalog.aclexplode(COALESCE(' || E'\n' ||
        '                namespace.nspacl,' || E'\n' ||
        '                pg_catalog.acldefault(''n'', namespace.nspowner)' || E'\n' ||
        '            )) AS privilege' || E'\n' ||
        '        ) <> 2' || E'\n' ||
        '        OR EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM pg_catalog.aclexplode(COALESCE(' || E'\n' ||
        '                namespace.nspacl,' || E'\n' ||
        '                pg_catalog.acldefault(''n'', namespace.nspowner)' || E'\n' ||
        '            )) AS privilege' || E'\n' ||
        '            WHERE privilege.grantee <> common_owner' || E'\n' ||
        '                OR privilege.grantor <> common_owner' || E'\n' ||
        '                OR privilege.privilege_type NOT IN (''USAGE'', ''CREATE'')' || E'\n' ||
        '                OR privilege.is_grantable' || E'\n' ||
        '        );' || E'\n' ||
        '' || E'\n' ||
        '    SELECT pg_catalog.count(*)' || E'\n' ||
        '    INTO unsafe_schema_count' || E'\n' ||
        '    FROM pg_catalog.pg_namespace AS namespace';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_readiness_schema_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF unexpected_capability_count <> 0' || E'\n' ||
        '        OR unsafe_schema_count <> 0';
    next_fragment :=
        '    IF unexpected_capability_count <> 0' || E'\n' ||
        '        OR invalid_private_schema_count <> 0' || E'\n' ||
        '        OR unsafe_schema_count <> 0';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_readiness_expectation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''58520c287c446b96198d624c9e10c4dc7e1ba8371cc721f6192a903685197476''::TEXT';
    next_fragment :=
        '''27ebe976c214377f71f62cf7d9c90be3009e3c331e395dff7d63c587513be167''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_readiness_digest_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    external_executor_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_manifest_acl_count BIGINT;
    invalid_function_count BIGINT;
    invalid_schema_count BIGINT;
    public_snapshot_mismatch_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO executor_grantee, external_executor_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM pg_temp.starring_runtime_builder_capability AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> CASE WHEN executor_grantee IS NULL THEN 1 ELSE 2 END
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee = common_owner
        ) <> 1
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE WHEN executor_grantee IS NULL THEN 0 ELSE 1 END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    privilege.grantee <> common_owner
                    AND (
                        executor_grantee IS NULL
                        OR privilege.grantee <> executor_grantee
                    )
                )
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_manifest_acl_count
    FROM (
        VALUES (
            'public.starring_runtime_execution_schema_manifest_v1()'
        )
    ) AS expected(function_identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_schema_count
    FROM (
        VALUES ('starring_runtime_private_v2')
    ) AS expected(schema_name)
    LEFT JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.nspname = expected.schema_name
    WHERE namespace.oid IS NULL
        OR namespace.nspowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
        ) <> 2
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type NOT IN ('USAGE', 'CREATE')
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)',
                'input_value text'::TEXT,
                'bytea'::TEXT,
                'plpgsql'::TEXT
            ),
            (
                'starring_runtime_private_v2.starring_runtime_framed_digest_v2(bytea,bytea)',
                'digest_domain bytea, canonical_payload bytea'::TEXT,
                'text'::TEXT,
                'sql'::TEXT
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)',
                'requested_operation_id text, requested_tenant_id text, requested_installation_id text, requested_deployment_id text, requested_expected_revision bigint, requested_slot_guild_id text, requested_slot_ruleset_key text, requested_target_guild_id text, requested_target_ruleset_key text, requested_target_version bigint, requested_target_content_hash text, requested_target_binding_revision bigint, requested_target_binding_fingerprint text, requested_mutation_kind text, requested_product_semantic_request_digest text'::TEXT,
                'bytea'::TEXT,
                'plpgsql'::TEXT
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(bytea)',
                'canonical_payload bytea'::TEXT,
                'text'::TEXT,
                'plpgsql'::TEXT
            ),
            (
                'starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)',
                'requested_intent_id text, requested_operation_id text, requested_tenant_id text, requested_installation_id text, requested_deployment_id text, requested_expected_revision bigint, requested_slot_guild_id text, requested_slot_ruleset_key text, requested_target_guild_id text, requested_target_ruleset_key text, requested_target_version bigint, requested_target_content_hash text, requested_target_binding_revision bigint, requested_target_binding_fingerprint text, requested_mutation_kind text, requested_product_semantic_request_digest text'::TEXT,
                'bytea'::TEXT,
                'plpgsql'::TEXT
            ),
            (
                'starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)',
                'canonical_payload bytea'::TEXT,
                'text'::TEXT,
                'plpgsql'::TEXT
            )
    ) AS expected(identity, arguments, result, language_name)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'i'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 's'
        OR function_row.prosecdef
        OR function_row.proretset
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO public_snapshot_mismatch_count
    FROM (
        SELECT
            snapshot.function_oid,
            snapshot.function_owner,
            snapshot.function_acl,
            function_row.oid AS observed_oid,
            function_row.proowner AS observed_owner,
            function_row.proacl AS observed_acl
        FROM pg_temp.starring_runtime_builder_function_snapshot AS snapshot
        FULL OUTER JOIN (
            SELECT function_row.oid, function_row.proowner, function_row.proacl
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'public'
        ) AS function_row
            ON function_row.oid = snapshot.function_oid
    ) AS comparison
    WHERE comparison.function_oid IS NULL
        OR comparison.observed_oid IS NULL
        OR comparison.function_owner IS DISTINCT FROM comparison.observed_owner
        OR comparison.function_acl IS DISTINCT FROM comparison.observed_acl;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR external_executor_count > 1
        OR executor_grantee = 0
        OR invalid_capability_acl_count <> 0
        OR invalid_manifest_acl_count <> 0
        OR invalid_schema_count <> 0
        OR invalid_function_count <> 0
        OR public_snapshot_mismatch_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '27ebe976c214377f71f62cf7d9c90be3009e3c331e395dff7d63c587513be167'
        OR readiness_digest IS DISTINCT FROM
            'c32a430e629c5603de09a15769b664bd533f3d4a86d5b26f514657ad63fc5eec'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_canonical_builders_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
