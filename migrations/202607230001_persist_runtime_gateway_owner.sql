SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_deployments,
    public.runtime_execution_mutation_markers
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
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
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_gateway_owner_observe_v1',
            'starring_runtime_gateway_owner_acquire_v1',
            'starring_runtime_gateway_owner_renew_v1',
            'starring_runtime_gateway_owner_release_v1',
            'validate_runtime_gateway_owner_transition',
            'reject_runtime_gateway_owner_delete'
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
        OR pg_catalog.to_regclass('public.runtime_gateway_owners') IS NOT NULL
        OR collision_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '2680d0c4d909e5019c9bedbebdbff7d082699df68404874c3bd49c28d3239b09'
        OR readiness_digest IS DISTINCT FROM
            'bcf5881f5b3ae919a3d6e29570b270dba1777627e4c16e3fa058750e2786a311'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_preflight_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_gateway_owners (
    gateway_shard_id TEXT PRIMARY KEY,
    process_instance_id TEXT,
    lease_epoch BIGINT NOT NULL,
    expected_build_revision TEXT,
    owner_revision BIGINT,
    expires_at TIMESTAMPTZ,
    CONSTRAINT runtime_gateway_owners_shard_check CHECK (
        gateway_shard_id = 'shard:0'
    ),
    CONSTRAINT runtime_gateway_owners_epoch_check CHECK (
        lease_epoch BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_gateway_owners_process_check CHECK (
        process_instance_id IS NULL
        OR process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_gateway_owners_build_check CHECK (
        expected_build_revision IS NULL
        OR expected_build_revision ~ '^[A-Za-z0-9_.:/-]{1,128}$'
    ),
    CONSTRAINT runtime_gateway_owners_revision_check CHECK (
        owner_revision IS NULL
        OR owner_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_gateway_owners_state_check CHECK (
        (
            process_instance_id IS NULL
            AND expected_build_revision IS NULL
            AND owner_revision IS NULL
            AND expires_at IS NULL
        )
        OR (
            process_instance_id IS NOT NULL
            AND expected_build_revision IS NOT NULL
            AND owner_revision IS NOT NULL
            AND expires_at IS NOT NULL
        )
    )
);

CREATE FUNCTION public.validate_runtime_gateway_owner_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    old_owned BOOLEAN;
    new_owned BOOLEAN;
BEGIN
    new_owned := NEW.process_instance_id IS NOT NULL;

    IF TG_OP = 'INSERT' THEN
        IF NOT new_owned
            OR NEW.lease_epoch <> 1
            OR NEW.owner_revision <> 1
        THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_gateway_owner_insert_invalid';
        END IF;
        RETURN NEW;
    END IF;

    old_owned := OLD.process_instance_id IS NOT NULL;

    IF NEW.gateway_shard_id IS DISTINCT FROM OLD.gateway_shard_id THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_gateway_owner_shard_transition_invalid';
    END IF;

    IF new_owned THEN
        IF NEW.lease_epoch IS DISTINCT FROM OLD.lease_epoch THEN
            IF OLD.lease_epoch = 9223372036854775807
                OR NEW.lease_epoch IS DISTINCT FROM OLD.lease_epoch + 1
                OR NEW.owner_revision <> 1
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = '23514',
                    MESSAGE = 'runtime_gateway_owner_successor_invalid';
            END IF;
        ELSIF NOT old_owned
            OR NEW.process_instance_id IS DISTINCT FROM OLD.process_instance_id
            OR NEW.expected_build_revision
                IS DISTINCT FROM OLD.expected_build_revision
            OR OLD.owner_revision = 9223372036854775807
            OR NEW.owner_revision IS DISTINCT FROM OLD.owner_revision + 1
        THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_gateway_owner_renewal_invalid';
        END IF;
        RETURN NEW;
    END IF;

    IF NOT old_owned
        OR NEW.lease_epoch IS DISTINCT FROM OLD.lease_epoch
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_gateway_owner_release_invalid';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_gateway_owners_validate_transition
BEFORE INSERT OR UPDATE ON public.runtime_gateway_owners
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_gateway_owner_transition();

CREATE FUNCTION public.reject_runtime_gateway_owner_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_gateway_owner_delete_rejected';
END;
$function$;

CREATE TRIGGER runtime_gateway_owners_reject_delete
BEFORE DELETE ON public.runtime_gateway_owners
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_gateway_owner_delete();

CREATE FUNCTION public.starring_runtime_gateway_owner_observe_v1(
    expected_gateway_shard_id TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    gateway_shard_id TEXT,
    process_instance_id TEXT,
    lease_epoch BIGINT,
    expected_build_revision TEXT,
    owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
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
    owner_row public.runtime_gateway_owners%ROWTYPE;
BEGIN
    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0' THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-gateway-owner-v1:' || expected_gateway_shard_id,
            0
        )
    );
    database_now := pg_catalog.clock_timestamp();

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    gateway_shard_id := expected_gateway_shard_id;
    IF NOT FOUND
        OR owner_row.process_instance_id IS NULL
        OR owner_row.expires_at <= database_now
    THEN
        outcome_name := 'unowned';
        process_instance_id := NULL;
        lease_epoch := NULL;
        expected_build_revision := NULL;
        owner_revision := NULL;
        expires_at := NULL;
    ELSE
        outcome_name := 'owned';
        process_instance_id := owner_row.process_instance_id;
        lease_epoch := owner_row.lease_epoch;
        expected_build_revision := owner_row.expected_build_revision;
        owner_revision := owner_row.owner_revision;
        expires_at := owner_row.expires_at;
    END IF;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_gateway_owner_acquire_v1(
    expected_gateway_shard_id TEXT,
    expected_process_instance_id TEXT,
    requested_build_revision TEXT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    gateway_shard_id TEXT,
    process_instance_id TEXT,
    lease_epoch BIGINT,
    expected_build_revision TEXT,
    owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
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
    owner_row public.runtime_gateway_owners%ROWTYPE;
BEGIN
    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0'
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-gateway-owner-v1:' || expected_gateway_shard_id,
            0
        )
    );
    database_now := pg_catalog.clock_timestamp();

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    IF NOT FOUND THEN
        INSERT INTO public.runtime_gateway_owners (
            gateway_shard_id,
            process_instance_id,
            lease_epoch,
            expected_build_revision,
            owner_revision,
            expires_at
        ) VALUES (
            expected_gateway_shard_id,
            expected_process_instance_id,
            1,
            requested_build_revision,
            1,
            database_now + requested_lease_milliseconds * INTERVAL '1 millisecond'
        )
        RETURNING * INTO owner_row;
        outcome_name := 'acquired';
    ELSIF owner_row.process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND owner_row.expected_build_revision
            IS NOT DISTINCT FROM requested_build_revision
        AND owner_row.expires_at > database_now
    THEN
        outcome_name := 'acquired';
    ELSIF owner_row.process_instance_id IS NOT NULL
        AND owner_row.expires_at > database_now
    THEN
        outcome_name := 'contended';
    ELSE
        IF owner_row.lease_epoch = 9223372036854775807 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_gateway_owner_epoch_exhausted';
        END IF;

        UPDATE public.runtime_gateway_owners AS owner
        SET process_instance_id = expected_process_instance_id,
            lease_epoch = owner_row.lease_epoch + 1,
            expected_build_revision = requested_build_revision,
            owner_revision = 1,
            expires_at = database_now
                + requested_lease_milliseconds * INTERVAL '1 millisecond'
        WHERE owner.gateway_shard_id = expected_gateway_shard_id
        RETURNING owner.* INTO owner_row;
        outcome_name := 'acquired';
    END IF;

    gateway_shard_id := owner_row.gateway_shard_id;
    process_instance_id := owner_row.process_instance_id;
    lease_epoch := owner_row.lease_epoch;
    expected_build_revision := owner_row.expected_build_revision;
    owner_revision := owner_row.owner_revision;
    expires_at := owner_row.expires_at;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_gateway_owner_renew_v1(
    expected_gateway_shard_id TEXT,
    expected_process_instance_id TEXT,
    expected_lease_epoch BIGINT,
    requested_build_revision TEXT,
    expected_owner_revision BIGINT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    gateway_shard_id TEXT,
    process_instance_id TEXT,
    lease_epoch BIGINT,
    expected_build_revision TEXT,
    owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
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
    owner_row public.runtime_gateway_owners%ROWTYPE;
BEGIN
    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0'
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR requested_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_owner_revision NOT BETWEEN 1 AND 9223372036854775807
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-gateway-owner-v1:' || expected_gateway_shard_id,
            0
        )
    );
    database_now := pg_catalog.clock_timestamp();

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    gateway_shard_id := expected_gateway_shard_id;
    IF FOUND
        AND owner_row.process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND owner_row.lease_epoch IS NOT DISTINCT FROM expected_lease_epoch
        AND owner_row.expected_build_revision
            IS NOT DISTINCT FROM requested_build_revision
        AND owner_row.owner_revision
            IS NOT DISTINCT FROM expected_owner_revision
        AND owner_row.expires_at > database_now
    THEN
        IF owner_row.owner_revision = 9223372036854775807 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_gateway_owner_revision_exhausted';
        END IF;

        UPDATE public.runtime_gateway_owners AS owner
        SET owner_revision = owner_row.owner_revision + 1,
            expires_at = database_now
                + requested_lease_milliseconds * INTERVAL '1 millisecond'
        WHERE owner.gateway_shard_id = expected_gateway_shard_id
        RETURNING owner.* INTO owner_row;
        outcome_name := 'renewed';
        process_instance_id := owner_row.process_instance_id;
        lease_epoch := owner_row.lease_epoch;
        expected_build_revision := owner_row.expected_build_revision;
        owner_revision := owner_row.owner_revision;
        expires_at := owner_row.expires_at;
    ELSE
        outcome_name := 'not_current';
        IF FOUND
            AND owner_row.process_instance_id IS NOT NULL
            AND owner_row.expires_at > database_now
        THEN
            process_instance_id := owner_row.process_instance_id;
            lease_epoch := owner_row.lease_epoch;
            expected_build_revision := owner_row.expected_build_revision;
            owner_revision := owner_row.owner_revision;
            expires_at := owner_row.expires_at;
        ELSE
            process_instance_id := NULL;
            lease_epoch := NULL;
            expected_build_revision := NULL;
            owner_revision := NULL;
            expires_at := NULL;
        END IF;
    END IF;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_gateway_owner_release_v1(
    expected_gateway_shard_id TEXT,
    expected_process_instance_id TEXT,
    expected_lease_epoch BIGINT,
    requested_build_revision TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    gateway_shard_id TEXT,
    process_instance_id TEXT,
    lease_epoch BIGINT,
    expected_build_revision TEXT,
    owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
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
    owner_row public.runtime_gateway_owners%ROWTYPE;
BEGIN
    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0'
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR requested_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
    THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-gateway-owner-v1:' || expected_gateway_shard_id,
            0
        )
    );
    database_now := pg_catalog.clock_timestamp();

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    gateway_shard_id := expected_gateway_shard_id;
    IF FOUND
        AND owner_row.process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND owner_row.lease_epoch IS NOT DISTINCT FROM expected_lease_epoch
        AND owner_row.expected_build_revision
            IS NOT DISTINCT FROM requested_build_revision
        AND owner_row.expires_at > database_now
    THEN
        UPDATE public.runtime_gateway_owners AS owner
        SET process_instance_id = NULL,
            expected_build_revision = NULL,
            owner_revision = NULL,
            expires_at = NULL
        WHERE owner.gateway_shard_id = expected_gateway_shard_id;
        outcome_name := 'released';
        process_instance_id := expected_process_instance_id;
        lease_epoch := expected_lease_epoch;
        expected_build_revision := requested_build_revision;
        owner_revision := NULL;
        expires_at := NULL;
    ELSE
        outcome_name := 'not_held';
        IF FOUND
            AND owner_row.process_instance_id IS NOT NULL
            AND owner_row.expires_at > database_now
        THEN
            process_instance_id := owner_row.process_instance_id;
            lease_epoch := owner_row.lease_epoch;
            expected_build_revision := owner_row.expected_build_revision;
            owner_revision := owner_row.owner_revision;
            expires_at := owner_row.expires_at;
        ELSE
            process_instance_id := NULL;
            lease_epoch := NULL;
            expected_build_revision := NULL;
            owner_revision := NULL;
            expires_at := NULL;
        END IF;
    END IF;
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON TABLE public.runtime_gateway_owners FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.validate_runtime_gateway_owner_transition()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_gateway_owner_delete()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_gateway_owner_observe_v1(TEXT)
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_gateway_owner_acquire_v1(TEXT,TEXT,TEXT,BIGINT)
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_gateway_owner_renew_v1(TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT)
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_gateway_owner_release_v1(TEXT,TEXT,BIGINT,TEXT)
FROM PUBLIC;

DO $execution_acl$
DECLARE
    common_owner OID;
    executor_grantee OID;
    grantee_count BIGINT;
    invalid_capability_count BIGINT;
    executor_name NAME;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(DISTINCT privilege.grantee),
        pg_catalog.min(privilege.grantee::BIGINT)::OID
    INTO grantee_count, executor_grantee
    FROM (
        VALUES
            ('public.starring_runtime_execution_database_readiness_v1()'),
            ('public.starring_runtime_execution_database_identity_v1()'),
            ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
            ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
            ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
            ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
            ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
            ('public.starring_runtime_execution_recover_stale_live_v1()'),
            ('public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)')
    ) AS expected(identity)
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_database_readiness_v1()'),
            ('public.starring_runtime_execution_database_identity_v1()'),
            ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
            ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
            ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
            ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
            ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
            ('public.starring_runtime_execution_recover_stale_live_v1()'),
            ('public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)')
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
        ) <> CASE WHEN executor_grantee IS NULL THEN 0 ELSE 1 END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    privilege.grantee IS DISTINCT FROM executor_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR grantee_count > 1
        OR invalid_capability_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_execution_acl_drift';
    END IF;

    IF executor_grantee IS NOT NULL THEN
        executor_name := pg_catalog.pg_get_userbyid(executor_grantee);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_gateway_owner_execution_acl_drift';
        END IF;
        FOREACH function_identity IN ARRAY ARRAY[
            'public.starring_runtime_gateway_owner_observe_v1(TEXT)',
            'public.starring_runtime_gateway_owner_acquire_v1(TEXT,TEXT,TEXT,BIGINT)',
            'public.starring_runtime_gateway_owner_renew_v1(TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT)',
            'public.starring_runtime_gateway_owner_release_v1(TEXT,TEXT,BIGINT,TEXT)'
        ]::TEXT[]
        LOOP
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %I',
                function_identity,
                executor_name
            );
        END LOOP;
    END IF;
END;
$execution_acl$;

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
        '(pg_catalog.to_regclass(''public.runtime_execution_mutation_markers'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_gateway_owners'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_execution_recover_stale_live_v1()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    next_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_execution_recover_stale_live_v1()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_observe_v1(text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        'RETURN observed_count = 472' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''86247ffa5c796b6c2c4e4edb6b7f4464b7a53fe51363285642c7c5e52056d48b'';';
    next_fragment :=
        'RETURN observed_count = 495' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''7853a26f4fca9cd45c863c17350d7d02ab31c2dc8c9f16828a039797e9eb9891'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_manifest_expectation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);
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
        '(''public.runtime_execution_mutation_markers''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_gateway_owners''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                FALSE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(' || E'\n' ||
        '        identity,' || E'\n' ||
        '        arguments,' || E'\n' ||
        '        result,' || E'\n' ||
        '        language_name,' || E'\n' ||
        '        is_strict,' || E'\n' ||
        '        returns_set,' || E'\n' ||
        '        rows_estimate' || E'\n' ||
        '    )';
    next_fragment :=
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                FALSE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_observe_v1(text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, requested_build_revision text, requested_lease_milliseconds bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, requested_build_revision text, expected_owner_revision bigint, requested_lease_milliseconds bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, requested_build_revision text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(' || E'\n' ||
        '        identity,' || E'\n' ||
        '        arguments,' || E'\n' ||
        '        result,' || E'\n' ||
        '        language_name,' || E'\n' ||
        '        is_strict,' || E'\n' ||
        '        returns_set,' || E'\n' ||
        '        rows_estimate' || E'\n' ||
        '    )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '''2680d0c4d909e5019c9bedbebdbff7d082699df68404874c3bd49c28d3239b09''::TEXT';
    next_fragment :=
        '''4b7a0b8daf9868d92edfae0cd83e35d805d27b824ef04a8b4eb06a229caeedf0''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '(''public.reject_runtime_execution_mutation_marker_delete()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    next_fragment :=
        '(''public.reject_runtime_execution_mutation_marker_delete()''),' || E'\n' ||
        '            (''public.validate_runtime_gateway_owner_transition()''),' || E'\n' ||
        '            (''public.reject_runtime_gateway_owner_delete()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_readiness_protected_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_observe_v1(text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_readiness_allowlist_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);
    EXECUTE definition;
END;
$patch_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.min(privilege.grantee::BIGINT)::OID
    INTO executor_grantee
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
    INTO invalid_relation_count
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_gateway_owners')
        AND (
            relation.relkind <> 'r'
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
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_attribute AS attribute
                CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl)
                    AS privilege
                WHERE attribute.attrelid = relation.oid
                    AND attribute.attnum > 0
                    AND NOT attribute.attisdropped
                    AND privilege.grantee <> common_owner
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_gateway_owner_observe_v1(text)',
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)',
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)',
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)',
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.validate_runtime_gateway_owner_transition()',
                FALSE,
                FALSE,
                0::REAL,
                FALSE
            ),
            (
                'public.reject_runtime_gateway_owner_delete()',
                FALSE,
                FALSE,
                0::REAL,
                FALSE
            )
    ) AS expected(
        identity,
        is_strict,
        returns_set,
        rows_estimate,
        executor_capability
    )
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
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE
            WHEN expected.executor_capability AND executor_grantee IS NOT NULL
                THEN 1
            ELSE 0
        END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, executor_grantee)
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
        OR pg_catalog.to_regclass('public.runtime_gateway_owners') IS NULL
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '4b7a0b8daf9868d92edfae0cd83e35d805d27b824ef04a8b4eb06a229caeedf0'
        OR readiness_digest IS DISTINCT FROM
            '003baab6fe5443a3bcf6dc6356cd5595434ac68c507a56151a65874397432ff1'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_gateway_owner_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
