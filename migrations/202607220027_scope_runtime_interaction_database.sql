SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.automation_instances IN ACCESS EXCLUSIVE MODE;
LOCK TABLE
    public.product_control_plane_identity,
    public.automation_ruleset_versions
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    persistent_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    invalid_instance_count BIGINT;
    invalid_constraint_count BIGINT;
    unsafe_schema_create_count BIGINT;
    unsafe_default_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relpersistence = 'p'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, persistent_count, rls_disabled_count,
        owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.automation_instances')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 3
        OR ordinary_count <> 3
        OR persistent_count <> 3
        OR rls_disabled_count <> 3
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'runtime interaction relations are invalid'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'runtime interaction migration requires the common owner'
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

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE defaults.defaclnamespace IN (0, pg_catalog.to_regnamespace('public'))
        AND defaults.defaclrole = common_owner
        AND privilege.grantee <> defaults.defaclrole;

    IF unsafe_schema_create_count <> 0 OR unsafe_default_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction schema trust is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            ('public.automation_instances', 'automation_instances_pkey', 'p', 'PRIMARY KEY (guild_id, instance_id)', TRUE, 'CREATE UNIQUE INDEX automation_instances_pkey ON public.automation_instances USING btree (guild_id, instance_id)', 2, TRUE),
            ('public.automation_instances', 'automation_instances_instance_id_format', 'c', 'CHECK (instance_id ~ ''^[A-Za-z0-9_-]{1,32}$''::text)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_instances', 'automation_instances_resources_object', 'c', 'CHECK (jsonb_typeof(resources) = ''object''::text)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_instances', 'automation_instances_ruleset_version_valid', 'c', 'CHECK (ruleset_version >= 1 AND ruleset_version <= ''4294967295''::bigint)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_instances', 'automation_instances_status_valid', 'c', 'CHECK (status = ANY (ARRAY[''active''::text, ''deleting''::text, ''disabled''::text, ''deleted''::text]))', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_ruleset_versions', 'automation_ruleset_versions_pkey', 'p', 'PRIMARY KEY (guild_id, ruleset_key, version)', TRUE, 'CREATE UNIQUE INDEX automation_ruleset_versions_pkey ON public.automation_ruleset_versions USING btree (guild_id, ruleset_key, version)', 3, TRUE),
            ('public.automation_ruleset_versions', 'arv_hash_unique', 'u', 'UNIQUE (guild_id, ruleset_key, content_hash)', TRUE, 'CREATE UNIQUE INDEX arv_hash_unique ON public.automation_ruleset_versions USING btree (guild_id, ruleset_key, content_hash)', 3, FALSE),
            ('public.automation_ruleset_versions', 'arv_content_integrity', 'c', 'CHECK (canonical_content_hash IS NOT NULL AND canonical_content_hash = content_hash)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_ruleset_versions', 'arv_definition_object', 'c', 'CHECK (jsonb_typeof(definition) = ''object''::text)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_ruleset_versions', 'arv_hash_format', 'c', 'CHECK (content_hash ~ ''^[0-9a-f]{64}$''::text)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_ruleset_versions', 'arv_key_format', 'c', 'CHECK (ruleset_key ~ ''^[A-Za-z0-9_-]{1,64}$''::text)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_ruleset_versions', 'arv_schema_range', 'c', 'CHECK (schema_version >= 1 AND schema_version <= ''4294967295''::bigint)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.automation_ruleset_versions', 'arv_version_range', 'c', 'CHECK (version >= 1 AND version <= ''4294967295''::bigint)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.product_control_plane_identity', 'product_control_plane_identity_pkey', 'p', 'PRIMARY KEY (singleton)', TRUE, 'CREATE UNIQUE INDEX product_control_plane_identity_pkey ON public.product_control_plane_identity USING btree (singleton)', 1, TRUE),
            ('public.product_control_plane_identity', 'product_control_plane_identity_database_identity_key', 'u', 'UNIQUE (database_identity)', TRUE, 'CREATE UNIQUE INDEX product_control_plane_identity_database_identity_key ON public.product_control_plane_identity USING btree (database_identity)', 1, FALSE),
            ('public.product_control_plane_identity', 'product_control_plane_identity_nonzero', 'c', 'CHECK (database_identity <> ''00000000-0000-0000-0000-000000000000''::uuid)', FALSE, NULL::TEXT, 0, FALSE),
            ('public.product_control_plane_identity', 'product_control_plane_identity_singleton', 'c', 'CHECK (singleton)', FALSE, NULL::TEXT, 0, FALSE)
    ) AS expected(
        relation_identity,
        constraint_name,
        constraint_type,
        constraint_definition,
        no_inherit,
        index_definition,
        index_keys,
        is_primary
    )
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND constraint_row.conname = expected.constraint_name
    LEFT JOIN pg_catalog.pg_class AS index_row
        ON index_row.oid = constraint_row.conindid
    LEFT JOIN pg_catalog.pg_index AS index_contract
        ON index_contract.indexrelid = constraint_row.conindid
    LEFT JOIN pg_catalog.pg_am AS index_method
        ON index_method.oid = index_row.relam
    WHERE constraint_row.oid IS NULL
        OR constraint_row.connamespace <> pg_catalog.to_regnamespace('public')
        OR constraint_row.contype::TEXT IS DISTINCT FROM expected.constraint_type
        OR NOT constraint_row.convalidated
        OR constraint_row.condeferrable
        OR constraint_row.condeferred
        OR constraint_row.connoinherit IS DISTINCT FROM expected.no_inherit
        OR NOT constraint_row.conislocal
        OR constraint_row.coninhcount <> 0
        OR constraint_row.conparentid <> 0
        OR constraint_row.confrelid <> 0
        OR pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE)
            IS DISTINCT FROM expected.constraint_definition
        OR (constraint_row.conindid <> 0)
            IS DISTINCT FROM (expected.index_definition IS NOT NULL)
        OR (
            expected.index_definition IS NOT NULL
            AND (
                index_row.oid IS NULL
                OR index_row.relnamespace <> pg_catalog.to_regnamespace('public')
                OR index_row.relowner <> common_owner
                OR index_row.relkind <> 'i'
                OR index_row.relpersistence <> 'p'
                OR index_row.relispartition
                OR index_method.amname IS DISTINCT FROM 'btree'
                OR index_contract.indrelid
                    <> pg_catalog.to_regclass(expected.relation_identity)
                OR NOT index_contract.indisunique
                OR index_contract.indisprimary IS DISTINCT FROM expected.is_primary
                OR NOT index_contract.indisvalid
                OR NOT index_contract.indisready
                OR NOT index_contract.indislive
                OR NOT index_contract.indimmediate
                OR index_contract.indisclustered
                OR index_contract.indisreplident
                OR index_contract.indnullsnotdistinct
                OR index_contract.indnkeyatts <> expected.index_keys
                OR index_contract.indnatts <> expected.index_keys
                OR index_contract.indexprs IS NOT NULL
                OR index_contract.indpred IS NOT NULL
                OR pg_catalog.pg_get_indexdef(index_row.oid)
                    IS DISTINCT FROM expected.index_definition
            )
        );

    IF invalid_constraint_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_constraint AS constraint_row
            WHERE constraint_row.conrelid IN (
                pg_catalog.to_regclass('public.product_control_plane_identity'),
                pg_catalog.to_regclass('public.automation_instances'),
                pg_catalog.to_regclass('public.automation_ruleset_versions')
            )
        ) <> 17
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_index AS index_contract
            WHERE index_contract.indrelid IN (
                pg_catalog.to_regclass('public.product_control_plane_identity'),
                pg_catalog.to_regclass('public.automation_instances'),
                pg_catalog.to_regclass('public.automation_ruleset_versions')
            )
        ) <> 5
    THEN
        RAISE EXCEPTION 'runtime interaction relation constraints are invalid'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regprocedure('public.starring_ruleset_content_hash_v1(bigint,jsonb)')
            IS NULL
        OR pg_catalog.to_regprocedure('public.reject_ruleset_artifact_mutation()') IS NULL
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid
                    = pg_catalog.to_regclass('public.automation_ruleset_versions')
                AND trigger_row.tgname = 'automation_ruleset_versions_reject_mutation'
                AND trigger_row.tgfoid = pg_catalog.to_regprocedure(
                    'public.reject_ruleset_artifact_mutation()'
                )
                AND trigger_row.tgenabled = 'O'
                AND NOT trigger_row.tgisinternal
                AND trigger_row.tgtype = 26
                AND trigger_row.tgnargs = 0
                AND pg_catalog.octet_length(trigger_row.tgargs) = 0
                AND pg_catalog.octet_length(trigger_row.tgattr::TEXT) = 0
                AND trigger_row.tgqual IS NULL
                AND trigger_row.tgconstraint = 0
                AND NOT trigger_row.tgdeferrable
                AND NOT trigger_row.tginitdeferred
                AND trigger_row.tgoldtable IS NULL
                AND trigger_row.tgnewtable IS NULL
        )
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid
                    = pg_catalog.to_regclass('public.automation_ruleset_versions')
                AND trigger_row.tgname = 'automation_ruleset_versions_reject_truncate'
                AND trigger_row.tgfoid = pg_catalog.to_regprocedure(
                    'public.reject_ruleset_artifact_mutation()'
                )
                AND trigger_row.tgenabled = 'O'
                AND NOT trigger_row.tgisinternal
                AND trigger_row.tgtype = 34
                AND trigger_row.tgnargs = 0
                AND pg_catalog.octet_length(trigger_row.tgargs) = 0
                AND pg_catalog.octet_length(trigger_row.tgattr::TEXT) = 0
                AND trigger_row.tgqual IS NULL
                AND trigger_row.tgconstraint = 0
                AND NOT trigger_row.tgdeferrable
                AND NOT trigger_row.tginitdeferred
                AND trigger_row.tgoldtable IS NULL
                AND trigger_row.tgnewtable IS NULL
        )
    THEN
        RAISE EXCEPTION 'runtime interaction immutable RuleSet contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'guard_runtime_interaction_instance_mutation_v1',
            'starring_runtime_interaction_schema_manifest_v1',
            'starring_runtime_interaction_database_identity_v1',
            'starring_runtime_interaction_database_readiness_v1',
            'starring_runtime_interaction_route_read_v1',
            'starring_runtime_interaction_pinned_read_v1',
            'starring_runtime_interaction_instance_register_v1'
        );

    IF collision_count <> 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid = pg_catalog.to_regclass('public.automation_instances')
                AND trigger_row.tgname IN (
                    'automation_instances_guard_runtime_interaction_mutation',
                    'automation_instances_guard_runtime_interaction_truncate'
                )
                AND NOT trigger_row.tgisinternal
        )
    THEN
        RAISE EXCEPTION 'runtime interaction object name collision exists'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_instance_count
    FROM public.automation_instances AS instance
    LEFT JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = instance.guild_id
        AND version.ruleset_key = instance.ruleset_key
        AND version.version = instance.ruleset_version
    WHERE instance.guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(instance.guild_id) > 20
        OR (
            pg_catalog.length(instance.guild_id) = 20
            AND instance.guild_id > '18446744073709551615'
        )
        OR instance.created_by !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(instance.created_by) > 20
        OR (
            pg_catalog.length(instance.created_by) = 20
            AND instance.created_by > '18446744073709551615'
        )
        OR instance.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR pg_catalog.octet_length(instance.kind) NOT BETWEEN 1 AND 128
        OR pg_catalog.jsonb_typeof(instance.resources) <> 'object'
        OR pg_catalog.octet_length(instance.resources::TEXT) > 262144
        OR (
            instance.status = 'active'
            AND (
                version.guild_id IS NULL
                OR version.content_hash IS NULL
                OR version.canonical_content_hash IS DISTINCT FROM version.content_hash
                OR version.content_hash !~ '^[0-9a-f]{64}$'
                OR pg_catalog.jsonb_typeof(version.definition) <> 'object'
                OR pg_catalog.octet_length(version.definition::TEXT) > 524288
                OR version.created_by !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(version.created_by) > 20
                OR (
                    pg_catalog.length(version.created_by) = 20
                    AND version.created_by > '18446744073709551615'
                )
            )
        );

    IF invalid_instance_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction persisted instances are invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_interaction_schema_manifest_v1()
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    observed_count BIGINT;
    observed_digest TEXT;
BEGIN
    WITH manifest(value) AS (
        SELECT pg_catalog.concat_ws(
            '|',
            'constraint',
            pg_catalog.format(
                '%I.%I',
                relation_namespace.nspname,
                relation.relname
            ),
            constraint_row.conname,
            constraint_row.contype::TEXT,
            constraint_row.convalidated::TEXT,
            constraint_row.condeferrable::TEXT,
            constraint_row.condeferred::TEXT,
            constraint_row.connoinherit::TEXT,
            constraint_row.conislocal::TEXT,
            constraint_row.coninhcount::TEXT,
            (constraint_row.conparentid = 0)::TEXT,
            (constraint_row.confrelid = 0)::TEXT,
            COALESCE(index_row.relname, ''),
            pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE)
        )
        FROM pg_catalog.pg_constraint AS constraint_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = constraint_row.conrelid
        INNER JOIN pg_catalog.pg_namespace AS relation_namespace
            ON relation_namespace.oid = relation.relnamespace
        LEFT JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = constraint_row.conindid
        WHERE constraint_row.conrelid IN (
            pg_catalog.to_regclass('public.product_control_plane_identity'),
            pg_catalog.to_regclass('public.automation_instances'),
            pg_catalog.to_regclass('public.automation_ruleset_versions')
        )
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'index',
            pg_catalog.format(
                '%I.%I',
                table_namespace.nspname,
                table_row.relname
            ),
            pg_catalog.format(
                '%I.%I',
                index_namespace.nspname,
                index_row.relname
            ),
            (index_row.relowner = table_row.relowner)::TEXT,
            index_row.relkind::TEXT,
            index_row.relpersistence::TEXT,
            index_row.relispartition::TEXT,
            index_method.amname,
            index_contract.indisprimary::TEXT,
            index_contract.indisunique::TEXT,
            index_contract.indisvalid::TEXT,
            index_contract.indisready::TEXT,
            index_contract.indislive::TEXT,
            index_contract.indimmediate::TEXT,
            index_contract.indisclustered::TEXT,
            index_contract.indisreplident::TEXT,
            index_contract.indnullsnotdistinct::TEXT,
            index_contract.indnkeyatts::TEXT,
            index_contract.indnatts::TEXT,
            index_contract.indkey::TEXT,
            index_contract.indcollation::TEXT,
            index_contract.indclass::TEXT,
            index_contract.indoption::TEXT,
            COALESCE(
                pg_catalog.pg_get_expr(
                    index_contract.indexprs,
                    index_contract.indrelid
                ),
                ''
            ),
            COALESCE(
                pg_catalog.pg_get_expr(
                    index_contract.indpred,
                    index_contract.indrelid
                ),
                ''
            ),
            pg_catalog.pg_get_indexdef(index_row.oid)
        )
        FROM pg_catalog.pg_index AS index_contract
        INNER JOIN pg_catalog.pg_class AS table_row
            ON table_row.oid = index_contract.indrelid
        INNER JOIN pg_catalog.pg_namespace AS table_namespace
            ON table_namespace.oid = table_row.relnamespace
        INNER JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = index_contract.indexrelid
        INNER JOIN pg_catalog.pg_namespace AS index_namespace
            ON index_namespace.oid = index_row.relnamespace
        INNER JOIN pg_catalog.pg_am AS index_method
            ON index_method.oid = index_row.relam
        WHERE index_contract.indrelid IN (
            pg_catalog.to_regclass('public.product_control_plane_identity'),
            pg_catalog.to_regclass('public.automation_instances'),
            pg_catalog.to_regclass('public.automation_ruleset_versions')
        )
    )
    SELECT pg_catalog.count(*),
        pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.string_agg(value, E'\n' ORDER BY value),
                    'UTF8'
                )
            ),
            'hex'
        )
    INTO observed_count, observed_digest
    FROM manifest;

    RETURN observed_count = 22
        AND observed_digest
            = '5b4f6fd061991332c8b86244e1a906a07f73b29578336722f7161bd9dac7a61d';
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_instance_mutation_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP <> 'UPDATE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_instance_destructive_mutation_rejected';
    END IF;

    IF ROW(
            NEW.guild_id,
            NEW.instance_id,
            NEW.ruleset_key,
            NEW.ruleset_version,
            NEW.kind,
            NEW.created_by,
            NEW.resources
        ) IS DISTINCT FROM ROW(
            OLD.guild_id,
            OLD.instance_id,
            OLD.ruleset_key,
            OLD.ruleset_version,
            OLD.kind,
            OLD.created_by,
            OLD.resources
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_instance_identity_mutation_rejected';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE TRIGGER automation_instances_guard_runtime_interaction_mutation
BEFORE UPDATE OR DELETE ON public.automation_instances
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_instance_mutation_v1();

CREATE TRIGGER automation_instances_guard_runtime_interaction_truncate
BEFORE TRUNCATE ON public.automation_instances
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_instance_mutation_v1();

CREATE FUNCTION public.starring_runtime_interaction_route_read_v1(
    expected_guild_id TEXT,
    expected_instance_id TEXT
)
RETURNS TABLE(
    guild_id TEXT,
    instance_id TEXT,
    ruleset_key TEXT,
    ruleset_version BIGINT,
    kind TEXT,
    created_by TEXT,
    status TEXT,
    resources JSONB
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    instance_row public.automation_instances%ROWTYPE;
BEGIN
    IF expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_route_input';
    END IF;

    SELECT instance.*
    INTO instance_row
    FROM public.automation_instances AS instance
    WHERE instance.guild_id = expected_guild_id
        AND instance.instance_id = expected_instance_id;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    IF instance_row.guild_id IS DISTINCT FROM expected_guild_id
        OR instance_row.instance_id IS DISTINCT FROM expected_instance_id
        OR instance_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR instance_row.ruleset_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.octet_length(instance_row.kind) NOT BETWEEN 1 AND 128
        OR instance_row.created_by !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(instance_row.created_by) > 20
        OR (
            pg_catalog.length(instance_row.created_by) = 20
            AND instance_row.created_by > '18446744073709551615'
        )
        OR instance_row.status NOT IN ('active', 'deleting', 'disabled', 'deleted')
        OR pg_catalog.jsonb_typeof(instance_row.resources) <> 'object'
        OR pg_catalog.octet_length(instance_row.resources::TEXT) > 262144
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_persisted_route_invalid';
    END IF;

    guild_id := instance_row.guild_id;
    instance_id := instance_row.instance_id;
    ruleset_key := instance_row.ruleset_key;
    ruleset_version := instance_row.ruleset_version;
    kind := instance_row.kind;
    created_by := instance_row.created_by;
    status := instance_row.status;
    resources := instance_row.resources;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_pinned_read_v1(
    expected_guild_id TEXT,
    expected_instance_id TEXT
)
RETURNS TABLE(
    guild_id TEXT,
    instance_id TEXT,
    ruleset_key TEXT,
    ruleset_version BIGINT,
    kind TEXT,
    created_by TEXT,
    status TEXT,
    resources JSONB,
    artifact_found BOOLEAN,
    artifact_schema_version BIGINT,
    artifact_definition JSONB,
    artifact_content_hash TEXT,
    artifact_created_by TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    joined_row RECORD;
BEGIN
    IF expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_pinned_input';
    END IF;

    SELECT instance.guild_id,
        instance.instance_id,
        instance.ruleset_key,
        instance.ruleset_version,
        instance.kind,
        instance.created_by,
        instance.status,
        instance.resources,
        version.guild_id AS artifact_guild_id,
        version.ruleset_key AS artifact_ruleset_key,
        version.version AS artifact_version,
        version.schema_version AS artifact_schema_version,
        version.definition AS artifact_definition,
        version.content_hash AS artifact_content_hash,
        version.canonical_content_hash AS artifact_canonical_content_hash,
        version.created_by AS artifact_created_by
    INTO joined_row
    FROM public.automation_instances AS instance
    LEFT JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = instance.guild_id
        AND version.ruleset_key = instance.ruleset_key
        AND version.version = instance.ruleset_version
    WHERE instance.guild_id = expected_guild_id
        AND instance.instance_id = expected_instance_id;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    IF joined_row.guild_id IS DISTINCT FROM expected_guild_id
        OR joined_row.instance_id IS DISTINCT FROM expected_instance_id
        OR joined_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR joined_row.ruleset_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.octet_length(joined_row.kind) NOT BETWEEN 1 AND 128
        OR joined_row.created_by !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(joined_row.created_by) > 20
        OR (
            pg_catalog.length(joined_row.created_by) = 20
            AND joined_row.created_by > '18446744073709551615'
        )
        OR joined_row.status NOT IN ('active', 'deleting', 'disabled', 'deleted')
        OR pg_catalog.jsonb_typeof(joined_row.resources) <> 'object'
        OR pg_catalog.octet_length(joined_row.resources::TEXT) > 262144
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_persisted_instance_invalid';
    END IF;

    artifact_found := joined_row.artifact_guild_id IS NOT NULL;
    IF joined_row.status = 'active' AND artifact_found AND (
        joined_row.artifact_guild_id IS DISTINCT FROM joined_row.guild_id
        OR joined_row.artifact_ruleset_key IS DISTINCT FROM joined_row.ruleset_key
        OR joined_row.artifact_version IS DISTINCT FROM joined_row.ruleset_version
        OR joined_row.artifact_schema_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.jsonb_typeof(joined_row.artifact_definition) <> 'object'
        OR pg_catalog.octet_length(joined_row.artifact_definition::TEXT) > 524288
        OR joined_row.artifact_content_hash !~ '^[0-9a-f]{64}$'
        OR joined_row.artifact_canonical_content_hash
            IS DISTINCT FROM joined_row.artifact_content_hash
        OR joined_row.artifact_created_by !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(joined_row.artifact_created_by) > 20
        OR (
            pg_catalog.length(joined_row.artifact_created_by) = 20
            AND joined_row.artifact_created_by > '18446744073709551615'
        )
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_pinned_artifact_invalid';
    END IF;

    IF joined_row.status = 'active' AND NOT artifact_found AND (
        joined_row.artifact_ruleset_key IS NOT NULL
        OR joined_row.artifact_version IS NOT NULL
        OR joined_row.artifact_schema_version IS NOT NULL
        OR joined_row.artifact_definition IS NOT NULL
        OR joined_row.artifact_content_hash IS NOT NULL
        OR joined_row.artifact_canonical_content_hash IS NOT NULL
        OR joined_row.artifact_created_by IS NOT NULL
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_partial_pinned_artifact';
    END IF;

    guild_id := joined_row.guild_id;
    instance_id := joined_row.instance_id;
    ruleset_key := joined_row.ruleset_key;
    ruleset_version := joined_row.ruleset_version;
    kind := joined_row.kind;
    created_by := joined_row.created_by;
    status := joined_row.status;
    resources := joined_row.resources;
    artifact_schema_version := joined_row.artifact_schema_version;
    artifact_definition := joined_row.artifact_definition;
    artifact_content_hash := joined_row.artifact_content_hash;
    artifact_created_by := joined_row.artifact_created_by;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_instance_register_v1(
    expected_guild_id TEXT,
    expected_instance_id TEXT,
    expected_ruleset_key TEXT,
    expected_ruleset_version BIGINT,
    expected_kind TEXT,
    expected_created_by TEXT,
    expected_resources JSONB
)
RETURNS TEXT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    artifact_row RECORD;
    instance_row public.automation_instances%ROWTYPE;
    inserted_instance_id TEXT;
    invalid_resource_count BIGINT;
BEGIN
    IF expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_ruleset_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.octet_length(expected_kind) NOT BETWEEN 1 AND 128
        OR expected_created_by !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_created_by) > 20
        OR (
            pg_catalog.length(expected_created_by) = 20
            AND expected_created_by > '18446744073709551615'
        )
        OR pg_catalog.jsonb_typeof(expected_resources) <> 'object'
        OR pg_catalog.octet_length(expected_resources::TEXT) > 262144
        OR expected_resources - ARRAY['roles', 'channels', 'messages']::TEXT[] <> '{}'::JSONB
        OR (
            expected_resources ? 'roles'
            AND pg_catalog.jsonb_typeof(expected_resources -> 'roles') <> 'object'
        )
        OR (
            expected_resources ? 'channels'
            AND pg_catalog.jsonb_typeof(expected_resources -> 'channels') <> 'object'
        )
        OR (
            expected_resources ? 'messages'
            AND pg_catalog.jsonb_typeof(expected_resources -> 'messages') <> 'object'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_registration_input';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_resource_count
    FROM (
        SELECT role.key, role.value, 'id'::TEXT AS resource_kind
        FROM pg_catalog.jsonb_each(COALESCE(
            expected_resources -> 'roles',
            '{}'::JSONB
        )) AS role(key, value)
        UNION ALL
        SELECT channel.key, channel.value, 'id'::TEXT AS resource_kind
        FROM pg_catalog.jsonb_each(COALESCE(
            expected_resources -> 'channels',
            '{}'::JSONB
        )) AS channel(key, value)
        UNION ALL
        SELECT message.key, message.value, 'message'::TEXT AS resource_kind
        FROM pg_catalog.jsonb_each(COALESCE(
            expected_resources -> 'messages',
            '{}'::JSONB
        )) AS message(key, value)
    ) AS resource
    WHERE pg_catalog.octet_length(resource.key) NOT BETWEEN 1 AND 128
        OR (
            resource.resource_kind = 'id'
            AND (
                pg_catalog.jsonb_typeof(resource.value) <> 'string'
                OR resource.value #>> '{}' !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(resource.value #>> '{}') > 20
                OR (
                    pg_catalog.length(resource.value #>> '{}') = 20
                    AND resource.value #>> '{}' > '18446744073709551615'
                )
            )
        )
        OR (
            resource.resource_kind = 'message'
            AND (
                pg_catalog.jsonb_typeof(resource.value) <> 'object'
                OR resource.value - ARRAY['channel', 'id']::TEXT[] <> '{}'::JSONB
                OR NOT resource.value ?& ARRAY['channel', 'id']::TEXT[]
                OR pg_catalog.jsonb_typeof(resource.value -> 'channel') <> 'string'
                OR pg_catalog.jsonb_typeof(resource.value -> 'id') <> 'string'
                OR resource.value ->> 'channel' !~ '^[1-9][0-9]{0,19}$'
                OR resource.value ->> 'id' !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(resource.value ->> 'channel') > 20
                OR pg_catalog.length(resource.value ->> 'id') > 20
                OR (
                    pg_catalog.length(resource.value ->> 'channel') = 20
                    AND resource.value ->> 'channel' > '18446744073709551615'
                )
                OR (
                    pg_catalog.length(resource.value ->> 'id') = 20
                    AND resource.value ->> 'id' > '18446744073709551615'
                )
            )
        );

    IF invalid_resource_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_registration_resources';
    END IF;

    SELECT version.guild_id,
        version.ruleset_key,
        version.version,
        version.schema_version,
        version.definition,
        version.content_hash,
        version.canonical_content_hash,
        version.created_by
    INTO artifact_row
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = expected_guild_id
        AND version.ruleset_key = expected_ruleset_key
        AND version.version = expected_ruleset_version
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_registration_artifact_missing';
    END IF;

    IF artifact_row.guild_id IS DISTINCT FROM expected_guild_id
        OR artifact_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR artifact_row.version IS DISTINCT FROM expected_ruleset_version
        OR artifact_row.schema_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.jsonb_typeof(artifact_row.definition) <> 'object'
        OR pg_catalog.octet_length(artifact_row.definition::TEXT) > 524288
        OR artifact_row.content_hash !~ '^[0-9a-f]{64}$'
        OR artifact_row.canonical_content_hash IS DISTINCT FROM artifact_row.content_hash
        OR artifact_row.created_by !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(artifact_row.created_by) > 20
        OR (
            pg_catalog.length(artifact_row.created_by) = 20
            AND artifact_row.created_by > '18446744073709551615'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_registration_artifact_invalid';
    END IF;

    INSERT INTO public.automation_instances (
        guild_id,
        instance_id,
        ruleset_key,
        ruleset_version,
        kind,
        created_by,
        status,
        resources
    ) VALUES (
        expected_guild_id,
        expected_instance_id,
        expected_ruleset_key,
        expected_ruleset_version,
        expected_kind,
        expected_created_by,
        'active',
        expected_resources
    )
    ON CONFLICT (guild_id, instance_id) DO NOTHING
    RETURNING instance_id INTO inserted_instance_id;

    IF inserted_instance_id IS NOT NULL THEN
        RETURN 'created';
    END IF;

    SELECT instance.*
    INTO instance_row
    FROM public.automation_instances AS instance
    WHERE instance.guild_id = expected_guild_id
        AND instance.instance_id = expected_instance_id
    FOR SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_registration_conflict_row_missing';
    END IF;

    IF instance_row.ruleset_key IS NOT DISTINCT FROM expected_ruleset_key
        AND instance_row.ruleset_version IS NOT DISTINCT FROM expected_ruleset_version
        AND instance_row.kind IS NOT DISTINCT FROM expected_kind
        AND instance_row.created_by IS NOT DISTINCT FROM expected_created_by
        AND instance_row.status = 'active'
        AND instance_row.resources IS NOT DISTINCT FROM expected_resources
    THEN
        RETURN 'exact_replay';
    END IF;

    RETURN 'conflict';
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_database_identity_v1()
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
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_database_readiness_v1()
RETURNS TABLE(
    database_identity TEXT,
    database_name TEXT,
    executor_role TEXT,
    checked_at TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    common_owner OID;
    database_owner OID;
    database_oid OID;
    invoker_oid OID;
    invalid_relation_count BIGINT;
    invalid_attribute_count BIGINT;
    invalid_function_count BIGINT;
    invalid_support_function_count BIGINT;
    invalid_trigger_count BIGINT;
    identity_count BIGINT;
    unexpected_capability_count BIGINT;
    unsafe_schema_count BIGINT;
    unsafe_default_count BIGINT;
    role_found BOOLEAN;
    role_row RECORD;
BEGIN
    IF pg_catalog.current_setting('role') <> 'none' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_role_drift';
    END IF;

    invoker_oid := pg_catalog.to_regrole(session_user);
    SELECT role.rolsuper,
        role.rolinherit,
        role.rolcreaterole,
        role.rolcreatedb,
        role.rolcanlogin,
        role.rolreplication,
        role.rolbypassrls,
        role.rolconnlimit,
        role.rolconfig
    INTO role_row
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = invoker_oid;
    role_found := FOUND;

    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');

    SELECT database_row.oid, database_row.datdba
    INTO database_oid, database_owner
    FROM pg_catalog.pg_database AS database_row
    WHERE database_row.datname = pg_catalog.current_database();

    IF NOT FOUND
        OR NOT role_found
        OR invoker_oid IS NULL
        OR common_owner IS NULL
        OR database_oid IS NULL
        OR database_owner IS NULL
        OR invoker_oid IN (common_owner, database_owner)
        OR role_row.rolsuper
        OR role_row.rolinherit
        OR role_row.rolcreaterole
        OR role_row.rolcreatedb
        OR NOT role_row.rolcanlogin
        OR role_row.rolreplication
        OR role_row.rolbypassrls
        OR role_row.rolconnlimit NOT BETWEEN 1 AND 4
        OR COALESCE(pg_catalog.cardinality(role_row.rolconfig), 0) <> 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = invoker_oid
                OR membership.roleid = invoker_oid
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE (
                    setting.setrole = invoker_oid
                    AND setting.setdatabase IN (0, database_oid)
                )
                OR (
                    setting.setrole = 0
                    AND setting.setdatabase = database_oid
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_role_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.automation_instances'),
            ('public.automation_ruleset_versions')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE privilege.grantee <> relation.relowner
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE attribute.attrelid = relation.oid
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> relation.relowner
        );

    IF invalid_relation_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_schema_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_attribute_count
    FROM (
        VALUES
            ('public.product_control_plane_identity', 'singleton', 'boolean', TRUE, ''),
            ('public.product_control_plane_identity', 'database_identity', 'uuid', TRUE, ''),
            ('public.product_control_plane_identity', 'created_at', 'timestamp with time zone', TRUE, ''),
            ('public.automation_instances', 'guild_id', 'text', TRUE, ''),
            ('public.automation_instances', 'instance_id', 'text', TRUE, ''),
            ('public.automation_instances', 'ruleset_key', 'text', TRUE, ''),
            ('public.automation_instances', 'kind', 'text', TRUE, ''),
            ('public.automation_instances', 'created_by', 'text', TRUE, ''),
            ('public.automation_instances', 'status', 'text', TRUE, ''),
            ('public.automation_instances', 'resources', 'jsonb', TRUE, ''),
            ('public.automation_instances', 'ruleset_version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'guild_id', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'ruleset_key', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'schema_version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'definition', 'jsonb', TRUE, ''),
            ('public.automation_ruleset_versions', 'content_hash', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'created_by', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'canonical_content_hash', 'text', FALSE, 's')
    ) AS expected(relation_identity, attribute_name, type_name, is_not_null, generated_kind)
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND attribute.attname = expected.attribute_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attnum IS NULL
        OR pg_catalog.format_type(attribute.atttypid, attribute.atttypmod)
            IS DISTINCT FROM expected.type_name
        OR attribute.attnotnull IS DISTINCT FROM expected.is_not_null
        OR attribute.attgenerated IS DISTINCT FROM expected.generated_kind;

    IF invalid_attribute_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_attribute AS attribute
            WHERE attribute.attrelid IN (
                    pg_catalog.to_regclass('public.product_control_plane_identity'),
                    pg_catalog.to_regclass('public.automation_instances'),
                    pg_catalog.to_regclass('public.automation_ruleset_versions')
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
        ) <> 19
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_attribute_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_interaction_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_interaction_database_readiness_v1()',
                '',
                'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_route_read_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_pinned_read_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb, artifact_found boolean, artifact_schema_version bigint, artifact_definition jsonb, artifact_content_hash text, artifact_created_by text)',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_instance_register_v1(text,text,text,bigint,text,text,jsonb)',
                'expected_guild_id text, expected_instance_id text, expected_ruleset_key text, expected_ruleset_version bigint, expected_kind text, expected_created_by text, expected_resources jsonb',
                'text',
                FALSE,
                0::REAL
            )
    ) AS expected(identity, arguments, result, returns_set, result_rows)
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
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.result_rows
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM CASE
            WHEN expected.identity
                = 'public.starring_runtime_interaction_database_identity_v1()'
            THEN 'sql'
            ELSE 'plpgsql'
        END
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR NOT pg_catalog.has_function_privilege(invoker_oid, function_row.oid, 'EXECUTE')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, invoker_oid)
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR (
                    privilege.grantee = invoker_oid
                    AND privilege.grantor <> common_owner
                )
                OR (
                    privilege.grantee = common_owner
                    AND privilege.grantor <> common_owner
                )
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_support_function_count
    FROM (
        VALUES
            (
                'public.reject_ruleset_artifact_mutation()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.starring_runtime_interaction_schema_manifest_v1()',
                '',
                'boolean',
                TRUE
            )
    ) AS expected(identity, arguments, result, is_strict)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR pg_catalog.has_function_privilege(invoker_oid, function_row.oid, 'EXECUTE')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_support_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_immutability_drift';
    END IF;

    IF NOT public.starring_runtime_interaction_schema_manifest_v1() THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_constraint_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            (
                'public.automation_instances',
                'automation_instances_guard_runtime_interaction_mutation',
                'public.guard_runtime_interaction_instance_mutation_v1()',
                27
            ),
            (
                'public.automation_instances',
                'automation_instances_guard_runtime_interaction_truncate',
                'public.guard_runtime_interaction_instance_mutation_v1()',
                34
            ),
            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_mutation',
                'public.reject_ruleset_artifact_mutation()',
                26
            ),
            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_truncate',
                'public.reject_ruleset_artifact_mutation()',
                34
            )
    ) AS expected(relation_identity, trigger_name, function_identity, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND trigger_row.tgname = expected.trigger_name
        AND NOT trigger_row.tgisinternal
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(expected.function_identity)
        OR trigger_row.tgtype::INTEGER IS DISTINCT FROM expected.trigger_type
        OR trigger_row.tgnargs <> 0
        OR pg_catalog.octet_length(trigger_row.tgargs) <> 0
        OR pg_catalog.octet_length(trigger_row.tgattr::TEXT) <> 0
        OR trigger_row.tgqual IS NOT NULL
        OR trigger_row.tgconstraint <> 0
        OR trigger_row.tgdeferrable
        OR trigger_row.tginitdeferred
        OR trigger_row.tgoldtable IS NOT NULL
        OR trigger_row.tgnewtable IS NOT NULL;

    IF invalid_trigger_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_trigger_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_count
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
        AND (
            pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'CREATE')
            OR (
                namespace.nspname <> 'public'
                AND pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'USAGE')
            )
        );

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE privilege.grantee IN (0, invoker_oid);

    SELECT pg_catalog.count(*)
    INTO unexpected_capability_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE function_row.oid >= 16384
        AND pg_catalog.has_function_privilege(invoker_oid, function_row.oid, 'EXECUTE')
        AND function_row.oid NOT IN (
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_database_identity_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_database_readiness_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_route_read_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_pinned_read_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_register_v1(text,text,text,bigint,text,text,jsonb)'
            )
        )
        AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_';

    IF unexpected_capability_count <> 0
        OR unsafe_schema_count <> 0
        OR unsafe_default_count <> 0
        OR NOT pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CONNECT')
        OR NOT pg_catalog.has_schema_privilege(invoker_oid, 'public', 'USAGE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CREATE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'TEMPORARY')
        OR pg_catalog.has_schema_privilege(invoker_oid, 'public', 'CREATE')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_database AS database_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                database_row.datacl,
                pg_catalog.acldefault('d', database_row.datdba)
            )) AS privilege
            WHERE database_row.oid = database_oid
                AND privilege.grantee IN (0, invoker_oid)
                AND privilege.is_grantable
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE namespace.nspname = 'public'
                AND privilege.grantee IN (0, invoker_oid)
                AND privilege.is_grantable
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    namespace.nspowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR (
                                NOT (
                                    namespace.nspname = 'information_schema'
                                    AND privilege.privilege_type = 'USAGE'
                                )
                                AND NOT EXISTS (
                                    SELECT 1
                                    FROM pg_catalog.aclexplode(COALESCE(
                                        (
                                            SELECT initial.initprivs
                                            FROM pg_catalog.pg_init_privs AS initial
                                            WHERE initial.classoid
                                                    = 'pg_catalog.pg_namespace'::REGCLASS
                                                AND initial.objoid = namespace.oid
                                                AND initial.objsubid = 0
                                        ),
                                        pg_catalog.acldefault(
                                            'n',
                                            namespace.nspowner
                                        )
                                    )) AS initial_privilege
                                    WHERE initial_privilege.grantee = 0
                                        AND initial_privilege.privilege_type
                                            = privilege.privilege_type
                                )
                            )
                        )
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    relation.relowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR (
                                NOT (
                                    namespace.nspname = 'information_schema'
                                    AND privilege.privilege_type = 'SELECT'
                                )
                                AND NOT EXISTS (
                                    SELECT 1
                                    FROM pg_catalog.aclexplode(COALESCE(
                                        (
                                            SELECT initial.initprivs
                                            FROM pg_catalog.pg_init_privs AS initial
                                            WHERE initial.classoid
                                                    = 'pg_catalog.pg_class'::REGCLASS
                                                AND initial.objoid = relation.oid
                                                AND initial.objsubid = 0
                                        ),
                                        pg_catalog.acldefault(
                                            'r',
                                            relation.relowner
                                        )
                                    )) AS initial_privilege
                                    WHERE initial_privilege.grantee = 0
                                        AND initial_privilege.privilege_type
                                            = privilege.privilege_type
                                )
                            )
                        )
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND (
                    privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_class'::REGCLASS
                                            AND initial.objoid = relation.oid
                                            AND initial.objsubid = attribute.attnum
                                    ),
                                    pg_catalog.acldefault(
                                        'c',
                                        relation.relowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                sequence.relacl,
                pg_catalog.acldefault('s', sequence.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND sequence.relkind = 'S'
                AND (
                    sequence.relowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_class'::REGCLASS
                                            AND initial.objoid = sequence.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        's',
                                        sequence.relowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    function_row.proowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR function_row.oid >= 16384
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_proc'::REGCLASS
                                            AND initial.objoid = function_row.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        'f',
                                        function_row.proowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_type AS type_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = type_row.typnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                type_row.typacl,
                pg_catalog.acldefault('T', type_row.typowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    type_row.typowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR type_row.oid >= 16384
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_type'::REGCLASS
                                            AND initial.objoid = type_row.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        'T',
                                        type_row.typowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'SELECT')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'INSERT')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'UPDATE')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'DELETE')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'TRUNCATE')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'REFERENCES')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'TRIGGER')
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND (
                    pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'SELECT'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'INSERT'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'UPDATE'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'REFERENCES'
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND sequence.relkind = 'S'
                AND (
                    pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'USAGE')
                    OR pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'SELECT')
                    OR pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'UPDATE')
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege
            WHERE privilege.grantee IN (0, invoker_oid)
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_largeobject_metadata AS large_object
            WHERE large_object.lomowner = invoker_oid
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        large_object.lomacl,
                        pg_catalog.acldefault('L', large_object.lomowner)
                    )) AS privilege
                    WHERE privilege.grantee IN (0, invoker_oid)
                        AND (
                            privilege.privilege_type IN ('SELECT', 'UPDATE')
                            OR privilege.is_grantable
                        )
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_capability_drift';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.min(identity.database_identity::TEXT)
    INTO identity_count, database_identity
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.database_identity::TEXT
            ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        AND identity.created_at IS NOT NULL;

    IF identity_count <> 1 OR database_identity IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_identity_drift';
    END IF;

    database_name := pg_catalog.current_database()::TEXT;
    executor_role := session_user::TEXT;
    checked_at := pg_catalog.clock_timestamp();
    RETURN NEXT;
END;
$function$;

DO $privileges$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
    column_name NAME;
    relation_identity TEXT;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'runtime interaction owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOREACH relation_identity IN ARRAY ARRAY[
        'public.product_control_plane_identity',
        'public.automation_instances',
        'public.automation_ruleset_versions'
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON TABLE %s FROM PUBLIC CASCADE',
            relation_identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_class AS relation
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE relation.oid = pg_catalog.to_regclass(relation_identity)
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime interaction relation grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON TABLE %s FROM %I CASCADE',
                relation_identity,
                grantee_name
            );
        END LOOP;
        FOR column_name, grantee IN
            SELECT attribute.attname, privilege.grantee
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_attribute AS attribute
                ON attribute.attrelid = relation.oid
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE relation.oid = pg_catalog.to_regclass(relation_identity)
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime interaction column grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES (%I) ON TABLE %s FROM %I CASCADE',
                column_name,
                relation_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    FOREACH function_identity IN ARRAY ARRAY[
        'public.guard_runtime_interaction_instance_mutation_v1()',
        'public.reject_ruleset_artifact_mutation()',
        'public.starring_runtime_interaction_schema_manifest_v1()',
        'public.starring_runtime_interaction_database_identity_v1()',
        'public.starring_runtime_interaction_database_readiness_v1()',
        'public.starring_runtime_interaction_route_read_v1(TEXT,TEXT)',
        'public.starring_runtime_interaction_pinned_read_v1(TEXT,TEXT)',
        'public.starring_runtime_interaction_instance_register_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,JSONB)'
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_identity,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            function_identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = pg_catalog.to_regprocedure(function_identity)
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime interaction function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;
END;
$privileges$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_attribute_count BIGINT;
    invalid_function_count BIGINT;
    invalid_trigger_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.automation_instances'),
            ('public.automation_ruleset_versions')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE privilege.grantee <> relation.relowner
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE attribute.attrelid = relation.oid
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> relation.relowner
        );

    SELECT pg_catalog.count(*)
    INTO invalid_attribute_count
    FROM (
        VALUES
            ('public.product_control_plane_identity', 'singleton', 'boolean', TRUE, ''),
            ('public.product_control_plane_identity', 'database_identity', 'uuid', TRUE, ''),
            ('public.product_control_plane_identity', 'created_at', 'timestamp with time zone', TRUE, ''),
            ('public.automation_instances', 'guild_id', 'text', TRUE, ''),
            ('public.automation_instances', 'instance_id', 'text', TRUE, ''),
            ('public.automation_instances', 'ruleset_key', 'text', TRUE, ''),
            ('public.automation_instances', 'kind', 'text', TRUE, ''),
            ('public.automation_instances', 'created_by', 'text', TRUE, ''),
            ('public.automation_instances', 'status', 'text', TRUE, ''),
            ('public.automation_instances', 'resources', 'jsonb', TRUE, ''),
            ('public.automation_instances', 'ruleset_version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'guild_id', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'ruleset_key', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'schema_version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'definition', 'jsonb', TRUE, ''),
            ('public.automation_ruleset_versions', 'content_hash', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'created_by', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'canonical_content_hash', 'text', FALSE, 's')
    ) AS expected(relation_identity, attribute_name, type_name, is_not_null, generated_kind)
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND attribute.attname = expected.attribute_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attnum IS NULL
        OR pg_catalog.format_type(attribute.atttypid, attribute.atttypmod)
            IS DISTINCT FROM expected.type_name
        OR attribute.attnotnull IS DISTINCT FROM expected.is_not_null
        OR attribute.attgenerated IS DISTINCT FROM expected.generated_kind;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.guard_runtime_interaction_instance_mutation_v1()',
                '',
                'trigger',
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_interaction_schema_manifest_v1()',
                '',
                'boolean',
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.reject_ruleset_artifact_mutation()',
                '',
                'trigger',
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_interaction_database_identity_v1()',
                '',
                'text',
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_interaction_database_readiness_v1()',
                '',
                'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)',
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_route_read_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_pinned_read_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb, artifact_found boolean, artifact_schema_version bigint, artifact_definition jsonb, artifact_content_hash text, artifact_created_by text)',
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_instance_register_v1(text,text,text,bigint,text,text,jsonb)',
                'expected_guild_id text, expected_instance_id text, expected_ruleset_key text, expected_ruleset_version bigint, expected_kind text, expected_created_by text, expected_resources jsonb',
                'text',
                TRUE,
                FALSE,
                0::REAL
            )
    ) AS expected(identity, arguments, result, is_strict, returns_set, result_rows)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.result_rows
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM CASE
            WHEN expected.identity
                = 'public.starring_runtime_interaction_database_identity_v1()'
            THEN 'sql'
            ELSE 'plpgsql'
        END
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
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
    INTO invalid_trigger_count
    FROM (
        VALUES
            (
                'public.automation_instances',
                'automation_instances_guard_runtime_interaction_mutation',
                'public.guard_runtime_interaction_instance_mutation_v1()',
                27
            ),
            (
                'public.automation_instances',
                'automation_instances_guard_runtime_interaction_truncate',
                'public.guard_runtime_interaction_instance_mutation_v1()',
                34
            ),
            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_mutation',
                'public.reject_ruleset_artifact_mutation()',
                26
            ),
            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_truncate',
                'public.reject_ruleset_artifact_mutation()',
                34
            )
    ) AS expected(relation_identity, trigger_name, function_identity, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND trigger_row.tgname = expected.trigger_name
        AND NOT trigger_row.tgisinternal
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(expected.function_identity)
        OR trigger_row.tgtype::INTEGER IS DISTINCT FROM expected.trigger_type
        OR trigger_row.tgnargs <> 0
        OR pg_catalog.octet_length(trigger_row.tgargs) <> 0
        OR pg_catalog.octet_length(trigger_row.tgattr::TEXT) <> 0
        OR trigger_row.tgqual IS NOT NULL
        OR trigger_row.tgconstraint <> 0
        OR trigger_row.tgdeferrable
        OR trigger_row.tginitdeferred
        OR trigger_row.tgoldtable IS NOT NULL
        OR trigger_row.tgnewtable IS NOT NULL;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_attribute_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_attribute AS attribute
            WHERE attribute.attrelid IN (
                    pg_catalog.to_regclass('public.product_control_plane_identity'),
                    pg_catalog.to_regclass('public.automation_instances'),
                    pg_catalog.to_regclass('public.automation_ruleset_versions')
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
        ) <> 19
        OR invalid_function_count <> 0
        OR invalid_trigger_count <> 0
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION 'runtime interaction database contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
