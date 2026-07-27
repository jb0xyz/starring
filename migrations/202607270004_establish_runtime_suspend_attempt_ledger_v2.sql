SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_slot_writer_fences_v2,
    public.runtime_drain_intents_v2,
    public.runtime_deployments,
    public.runtime_certification_operations_v2,
    public.runtime_execution_mutation_markers,
    public.runtime_attestations,
    public.runtime_serving_leases,
    public.runtime_gateway_owners,
    public.automation_installations
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    executor_role OID;
    executor_role_is_quarantined BOOLEAN;
    executor_membership_count BIGINT;
    other_client_session_count BIGINT;
    prepared_transaction_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT privilege.grantee
    INTO executor_role
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
    ORDER BY privilege.grantee
    LIMIT 1;

    SELECT COALESCE(NOT role.rolcanlogin, TRUE)
    INTO executor_role_is_quarantined
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = executor_role;
    executor_role_is_quarantined := COALESCE(
        executor_role_is_quarantined,
        executor_role IS NULL
    );

    SELECT pg_catalog.count(*)
    INTO executor_membership_count
    FROM pg_catalog.pg_auth_members AS membership
    WHERE membership.roleid = executor_role
        OR membership.member = executor_role;

    SELECT pg_catalog.count(*)
    INTO other_client_session_count
    FROM pg_catalog.pg_stat_activity AS activity
    WHERE activity.datid = (
            SELECT database_row.oid
            FROM pg_catalog.pg_database AS database_row
            WHERE database_row.datname = pg_catalog.current_database()
        )
        AND activity.pid <> pg_catalog.pg_backend_pid()
        AND activity.backend_type = 'client backend';

    SELECT pg_catalog.count(*)
    INTO prepared_transaction_count
    FROM pg_catalog.pg_prepared_xacts AS prepared
    WHERE prepared.database = pg_catalog.current_database();

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
        OR NOT executor_role_is_quarantined
        OR executor_membership_count <> 0
        OR other_client_session_count <> 0
        OR prepared_transaction_count <> 0
        OR pg_catalog.to_regclass(
            'public.runtime_suspend_attempt_operations_v2'
        ) IS NOT NULL
        OR pg_catalog.to_regclass(
            'public.runtime_suspended_attempts_v2'
        ) IS NOT NULL
        OR pg_catalog.to_regclass(
            'public.runtime_suspend_attempt_completions_v2'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'public.reject_runtime_suspend_attempt_ledger_mutation_v2()'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'public.validate_runtime_suspend_attempt_ledger_v2()'
        ) IS NOT NULL
        OR EXISTS (
            SELECT 1
            FROM public.runtime_gateway_owners AS owner
            WHERE owner.process_instance_id IS NOT NULL
                AND owner.expires_at > pg_catalog.clock_timestamp()
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_serving_leases AS serving
            WHERE serving.connected
                AND serving.serving
                AND serving.expires_at > pg_catalog.clock_timestamp()
        )
        OR manifest_digest IS DISTINCT FROM
            'ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4'
        OR readiness_digest IS DISTINCT FROM
            'c5972296ea84090bae5708fc9efa90cd9f9f848acb156e40680c0ba04fb57b5c'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_preflight_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_suspend_attempt_operations_v2 (
    suspension_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    convergence_attempt_no BIGINT NOT NULL,
    suspend_attempt_request_bytes BYTEA NOT NULL,
    suspend_attempt_digest TEXT NOT NULL,
    CONSTRAINT runtime_suspend_attempt_operations_v2_scope_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id
    ) REFERENCES public.runtime_deployments (
        tenant_id,
        installation_id,
        deployment_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_suspend_attempt_operations_v2_natural_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ),
    CONSTRAINT runtime_suspend_attempt_operations_v2_child_unique UNIQUE (
        suspension_id,
        suspend_attempt_digest,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ),
    CONSTRAINT runtime_suspend_attempt_operations_v2_id_check CHECK (
        suspension_id ~ '^[0-9a-f]{32}$'
    ),
    CONSTRAINT runtime_suspend_attempt_operations_v2_scope_check CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_suspend_attempt_operations_v2_revision_check CHECK (
        deployment_revision BETWEEN 1 AND 9223372036854775807
        AND convergence_attempt_no BETWEEN 1 AND 4294967295
    ),
    CONSTRAINT runtime_suspend_attempt_operations_v2_canonical_check CHECK (
        pg_catalog.octet_length(suspend_attempt_request_bytes)
            BETWEEN 1 AND 131072
        AND suspend_attempt_digest ~ '^[0-9a-f]{64}$'
    )
);

CREATE TABLE public.runtime_suspended_attempts_v2 (
    suspension_id TEXT PRIMARY KEY,
    suspend_attempt_digest TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    convergence_attempt_no BIGINT NOT NULL,
    sidecar_revision BIGINT NOT NULL,
    slot_guild_id TEXT NOT NULL,
    slot_ruleset_key TEXT NOT NULL,
    local_effect_kind TEXT NOT NULL,
    local_effect_bytes BYTEA NOT NULL,
    drain_obligation_kind TEXT NOT NULL,
    drain_obligation_bytes BYTEA NOT NULL,
    suspended_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_suspended_attempts_v2_root_fk FOREIGN KEY (
        suspension_id,
        suspend_attempt_digest,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ) REFERENCES public.runtime_suspend_attempt_operations_v2 (
        suspension_id,
        suspend_attempt_digest,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_suspended_attempts_v2_slot_fk FOREIGN KEY (
        slot_guild_id,
        slot_ruleset_key
    ) REFERENCES public.automation_installations (
        discord_guild_id,
        ruleset_key
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_suspended_attempts_v2_slot_unique UNIQUE (
        slot_guild_id,
        slot_ruleset_key
    ),
    CONSTRAINT runtime_suspended_attempts_v2_revision_check CHECK (
        sidecar_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_suspended_attempts_v2_slot_check CHECK (
        slot_guild_id ~ '^[1-9][0-9]{0,19}$'
        AND (
            pg_catalog.length(slot_guild_id) < 20
            OR slot_guild_id COLLATE pg_catalog."C"
                <= '18446744073709551615' COLLATE pg_catalog."C"
        )
        AND slot_ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT runtime_suspended_attempts_v2_effect_check CHECK (
        local_effect_kind IN ('none', 'exact_route', 'route_absent')
        AND pg_catalog.octet_length(local_effect_bytes)
            BETWEEN 1 AND 131072
    ),
    CONSTRAINT runtime_suspended_attempts_v2_obligation_check CHECK (
        drain_obligation_kind IN (
            'none',
            'exact_local_route',
            'previous_serving',
            'local_and_previous'
        )
        AND pg_catalog.octet_length(drain_obligation_bytes)
            BETWEEN 1 AND 131072
    ),
    CONSTRAINT runtime_suspended_attempts_v2_time_check CHECK (
        pg_catalog.isfinite(suspended_at)
    )
);

CREATE INDEX runtime_suspended_attempts_v2_recovery_index
ON public.runtime_suspended_attempts_v2 (
    local_effect_kind,
    suspended_at,
    suspension_id
);

CREATE TABLE public.runtime_suspend_attempt_completions_v2 (
    suspension_id TEXT PRIMARY KEY,
    suspend_attempt_digest TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    convergence_attempt_no BIGINT NOT NULL,
    resulting_deployment_revision BIGINT NOT NULL,
    resulting_convergence_attempt_no BIGINT NOT NULL,
    successor_controller_id TEXT NOT NULL,
    successor_controller_fencing_token BIGINT NOT NULL,
    successor_acquired_at TIMESTAMPTZ NOT NULL,
    successor_expires_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_suspend_attempt_completions_v2_root_fk FOREIGN KEY (
        suspension_id,
        suspend_attempt_digest,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ) REFERENCES public.runtime_suspend_attempt_operations_v2 (
        suspension_id,
        suspend_attempt_digest,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_suspend_attempt_completions_v2_revision_check CHECK (
        deployment_revision < 9223372036854775807
        AND resulting_deployment_revision = deployment_revision + 1
        AND convergence_attempt_no < 4294967295
        AND resulting_convergence_attempt_no = convergence_attempt_no + 1
    ),
    CONSTRAINT runtime_suspend_attempt_completions_v2_controller_check CHECK (
        successor_controller_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND successor_controller_fencing_token
            BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_suspend_attempt_completions_v2_time_check CHECK (
        pg_catalog.isfinite(successor_acquired_at)
        AND pg_catalog.isfinite(successor_expires_at)
        AND pg_catalog.isfinite(completed_at)
        AND successor_expires_at > successor_acquired_at
        AND completed_at >= successor_acquired_at
    )
);

CREATE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_suspend_attempt_ledger_mutation_rejected';
END;
$function$;

CREATE FUNCTION public.validate_runtime_suspend_attempt_ledger_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    checked_suspension_id TEXT;
    root_count BIGINT;
    active_count BIGINT;
    completion_count BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        checked_suspension_id := OLD.suspension_id;
    ELSE
        checked_suspension_id := NEW.suspension_id;
    END IF;

    SELECT pg_catalog.count(*)
    INTO root_count
    FROM public.runtime_suspend_attempt_operations_v2 AS operation
    WHERE operation.suspension_id = checked_suspension_id;

    SELECT pg_catalog.count(*)
    INTO active_count
    FROM public.runtime_suspended_attempts_v2 AS suspended
    WHERE suspended.suspension_id = checked_suspension_id;

    SELECT pg_catalog.count(*)
    INTO completion_count
    FROM public.runtime_suspend_attempt_completions_v2 AS completion
    WHERE completion.suspension_id = checked_suspension_id;

    IF root_count <> 1
        OR active_count + completion_count <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_suspend_attempt_ledger_consistency_invalid';
    END IF;

    RETURN NULL;
END;
$function$;

CREATE TRIGGER runtime_suspend_attempt_operations_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE
ON public.runtime_suspend_attempt_operations_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2();

CREATE TRIGGER runtime_suspend_attempt_operations_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_suspend_attempt_operations_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2();

CREATE CONSTRAINT TRIGGER runtime_suspend_attempt_operations_v2_validate
AFTER INSERT OR UPDATE OR DELETE
ON public.runtime_suspend_attempt_operations_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_suspend_attempt_ledger_v2();

CREATE TRIGGER runtime_suspended_attempts_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE
ON public.runtime_suspended_attempts_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2();

CREATE TRIGGER runtime_suspended_attempts_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_suspended_attempts_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2();

CREATE CONSTRAINT TRIGGER runtime_suspended_attempts_v2_validate
AFTER INSERT OR UPDATE OR DELETE
ON public.runtime_suspended_attempts_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_suspend_attempt_ledger_v2();

CREATE TRIGGER runtime_suspend_attempt_completions_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE
ON public.runtime_suspend_attempt_completions_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2();

CREATE TRIGGER runtime_suspend_attempt_completions_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_suspend_attempt_completions_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2();

CREATE CONSTRAINT TRIGGER runtime_suspend_attempt_completions_v2_validate
AFTER INSERT OR UPDATE OR DELETE
ON public.runtime_suspend_attempt_completions_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_suspend_attempt_ledger_v2();

REVOKE ALL ON TABLE
    public.runtime_suspend_attempt_operations_v2,
    public.runtime_suspended_attempts_v2,
    public.runtime_suspend_attempt_completions_v2
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_suspend_attempt_ledger_mutation_v2()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.validate_runtime_suspend_attempt_ledger_v2()
FROM PUBLIC;

DO $patch_schema_manifest$
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

    previous_fragment :=
        '            (pg_catalog.to_regclass(''public.runtime_certification_operations_v2'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_suspend_attempt_operations_v2'')),' || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_suspended_attempts_v2'')),' || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_suspend_attempt_completions_v2'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 650' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''65c41a8e67ec225e567403f2f24eba8e31964a51d1a1ce484774cae3db5bd58c'';';
    next_fragment :=
        'RETURN observed_count = 733' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''984539f97c292c40c30b262087e312cd423d06c149fb30a4cba6af9596574120'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_manifest_expectation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_schema_manifest$;

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
        '            (''public.runtime_certification_operations_v2''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_suspend_attempt_operations_v2''),' || E'\n' ||
        '            (''public.runtime_suspended_attempts_v2''),' || E'\n' ||
        '            (''public.runtime_suspend_attempt_completions_v2''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'')';
    next_fragment :=
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''public.reject_runtime_suspend_attempt_ledger_mutation_v2()''),' || E'\n' ||
        '            (''public.validate_runtime_suspend_attempt_ledger_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4''::TEXT';
    next_fragment :=
        '''57694b2a5f374fa63882fb52f5bfe506b321968c961ea2cf9de8006fd46a5979''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_readiness_manifest_digest_patch_drift';
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
    executor_role OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_trigger_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT privilege.grantee
    INTO executor_role
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
    ORDER BY privilege.grantee
    LIMIT 1;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_suspend_attempt_operations_v2'),
            ('public.runtime_suspended_attempts_v2'),
            ('public.runtime_suspend_attempt_completions_v2')
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
            WHERE privilege.grantee <> common_owner
        )
        OR (
            executor_role IS NOT NULL
            AND (
                pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'SELECT'
                )
                OR pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'INSERT'
                )
                OR pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'UPDATE'
                )
                OR pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'DELETE'
                )
                OR pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'TRUNCATE'
                )
                OR pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'REFERENCES'
                )
                OR pg_catalog.has_table_privilege(
                    executor_role,
                    relation.oid,
                    'TRIGGER'
                )
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.reject_runtime_suspend_attempt_ledger_mutation_v2()'),
            ('public.validate_runtime_suspend_attempt_ledger_v2()')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR function_row.proretset
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
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
        )
        OR (
            executor_role IS NOT NULL
            AND pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            ('public.runtime_suspend_attempt_operations_v2', 'runtime_suspend_attempt_operations_v2_reject_row_mutation'),
            ('public.runtime_suspend_attempt_operations_v2', 'runtime_suspend_attempt_operations_v2_reject_truncate'),
            ('public.runtime_suspend_attempt_operations_v2', 'runtime_suspend_attempt_operations_v2_validate'),
            ('public.runtime_suspended_attempts_v2', 'runtime_suspended_attempts_v2_reject_row_mutation'),
            ('public.runtime_suspended_attempts_v2', 'runtime_suspended_attempts_v2_reject_truncate'),
            ('public.runtime_suspended_attempts_v2', 'runtime_suspended_attempts_v2_validate'),
            ('public.runtime_suspend_attempt_completions_v2', 'runtime_suspend_attempt_completions_v2_reject_row_mutation'),
            ('public.runtime_suspend_attempt_completions_v2', 'runtime_suspend_attempt_completions_v2_reject_truncate'),
            ('public.runtime_suspend_attempt_completions_v2', 'runtime_suspend_attempt_completions_v2_validate')
    ) AS expected(relation_identity, trigger_name)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            expected.relation_identity
        )
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O';

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
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_trigger_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_suspend_attempt_operations_v2
        ) <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_suspended_attempts_v2
        ) <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_suspend_attempt_completions_v2
        ) <> 0
        OR manifest_digest IS DISTINCT FROM
            '57694b2a5f374fa63882fb52f5bfe506b321968c961ea2cf9de8006fd46a5979'
        OR readiness_digest IS DISTINCT FROM
            '6523d219df9a148c9428ac8f45b9317bcad6b56af44b753f11167fc582ca5875'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_suspend_attempt_ledger_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
