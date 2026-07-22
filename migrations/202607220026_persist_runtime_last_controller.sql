SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.runtime_deployments, public.runtime_attestations, public.runtime_serving_leases
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    relation_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
    unsafe_default_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF relation_count <> 3
        OR owner_count <> 1
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'runtime last controller relations require their common owner'
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
        AND privilege.grantee <> defaults.defaclrole;

    IF unsafe_schema_create_count <> 0 OR unsafe_default_count <> 0 THEN
        RAISE EXCEPTION 'runtime last controller schema trust is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_attribute AS attribute
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = attribute.attrelid
    WHERE (
        (
                relation.oid = pg_catalog.to_regclass('public.runtime_deployments')
                AND attribute.attname = 'last_controller_id'
            ) OR (
                relation.oid = pg_catalog.to_regclass('public.runtime_attestations')
                AND attribute.attname = 'serving_lease_duration_nanos'
            )
        )
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    IF collision_count <> 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_constraint AS constraint_row
            WHERE constraint_row.conname IN (
                'runtime_deployments_last_controller_valid',
                'runtime_attestations_serving_lease_duration_valid'
            )
        )
    THEN
        RAISE EXCEPTION 'runtime last controller schema collides with an existing object'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regprocedure(
            'public.validate_runtime_convergence_attempt_projection()'
        ) IS NULL
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid = pg_catalog.to_regclass('public.runtime_deployments')
                AND trigger_row.tgname = 'runtime_deployments_validate_convergence_attempt'
                AND trigger_row.tgfoid = pg_catalog.to_regprocedure(
                    'public.validate_runtime_convergence_attempt_projection()'
                )
                AND trigger_row.tgenabled = 'O'
                AND NOT trigger_row.tgisinternal
        )
    THEN
        RAISE EXCEPTION 'runtime convergence attempt protection is unavailable'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.runtime_attestations
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_serving_leases
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        WHERE deployment.phase <> 'requested'
            OR deployment.revision <> 1
            OR deployment.convergence_attempt_no <> 0
            OR deployment.last_failure_attempt_no IS NOT NULL
            OR deployment.controller_id IS NOT NULL
            OR deployment.controller_fencing_token IS NOT NULL
            OR deployment.controller_acquired_at IS NOT NULL
            OR deployment.controller_lease_expires_at IS NOT NULL
            OR deployment.last_fencing_token IS NOT NULL
            OR deployment.next_retry_at IS NOT NULL
            OR deployment.last_stable_error_code IS NOT NULL
            OR deployment.live_attestation_id IS NOT NULL
            OR deployment.live_at IS NOT NULL
            OR deployment.blocked_at IS NOT NULL
            OR deployment.superseded_at IS NOT NULL
            OR deployment.cancelled_at IS NOT NULL
            OR deployment.snapshot -> 'controller_lease' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'last_fencing_token' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'preflight' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'drain' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'activation' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'panel_certificate' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'gateway_ready' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'live' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'last_live_recovery' IS DISTINCT FROM 'null'::JSONB
            OR deployment.snapshot -> 'last_runtime_failure' IS DISTINCT FROM 'null'::JSONB
    ) THEN
        RAISE EXCEPTION 'legacy runtime controller history cannot be inferred safely'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_deployments
ADD COLUMN last_controller_id TEXT;

ALTER TABLE public.runtime_deployments
ADD CONSTRAINT runtime_deployments_last_controller_valid CHECK (
    (
        (last_fencing_token IS NULL AND last_controller_id IS NULL)
        OR (
            last_fencing_token IS NOT NULL
            AND last_controller_id IS NOT NULL
            AND last_controller_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        )
    )
    AND (
        controller_id IS NULL
        OR last_controller_id IS NOT DISTINCT FROM controller_id
    )
) NOT VALID;

ALTER TABLE public.runtime_deployments
VALIDATE CONSTRAINT runtime_deployments_last_controller_valid;

ALTER TABLE public.runtime_attestations
ADD COLUMN serving_lease_duration_nanos BIGINT NOT NULL;

ALTER TABLE public.runtime_attestations
ADD CONSTRAINT runtime_attestations_serving_lease_duration_valid CHECK (
    serving_lease_duration_nanos BETWEEN 1 AND 9223372036854775807
) NOT VALID;

ALTER TABLE public.runtime_attestations
VALIDATE CONSTRAINT runtime_attestations_serving_lease_duration_valid;

CREATE OR REPLACE FUNCTION public.validate_runtime_convergence_attempt_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    mutation_clock TIMESTAMPTZ;
    failure_disposition TEXT;
    pending_condition TEXT;
    snapshot_failure_attempt BIGINT;
    lease_claimed BOOLEAN;
    operator_recovery BOOLEAN;
    failure_changed BOOLEAN;
BEGIN
    IF (NEW.last_fencing_token IS NULL) <> (NEW.last_controller_id IS NULL)
        OR (
            NEW.last_controller_id IS NOT NULL
            AND NEW.last_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        )
    THEN
        RAISE EXCEPTION 'runtime last controller projection is invalid'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.controller_id IS NOT NULL
        AND NEW.last_controller_id IS DISTINCT FROM NEW.controller_id
    THEN
        RAISE EXCEPTION 'runtime active controller differs from its durable identity'
            USING ERRCODE = '23514';
    END IF;

    mutation_clock := public.starring_runtime_current_mutation_clock();
    failure_disposition := NEW.snapshot #>> '{last_runtime_failure,disposition}';
    pending_condition := NEW.snapshot #>> '{phase,condition,condition}';
    snapshot_failure_attempt := CASE
        WHEN failure_disposition = 'retryable'
            AND NEW.snapshot #>> '{last_runtime_failure,attempt}' ~ '^[1-9][0-9]{0,9}$'
        THEN (NEW.snapshot #>> '{last_runtime_failure,attempt}')::BIGINT
    END;

    IF failure_disposition IS NULL THEN
        IF NEW.last_failure_attempt_no IS NOT NULL THEN
            RAISE EXCEPTION 'runtime failure attempt exists without failure evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSIF failure_disposition = 'retryable' THEN
        IF snapshot_failure_attempt IS DISTINCT FROM NEW.last_failure_attempt_no THEN
            RAISE EXCEPTION 'runtime retry attempt differs from its durable attempt'
                USING ERRCODE = '23514';
        END IF;
    ELSIF failure_disposition = 'blocked' THEN
        IF NEW.last_failure_attempt_no IS NULL THEN
            RAISE EXCEPTION 'runtime blocked failure lacks its durable attempt'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        RAISE EXCEPTION 'runtime failure disposition is invalid'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.convergence_attempt_no = 0 THEN
        IF NEW.phase <> 'requested'
            OR NEW.revision <> 1
            OR NEW.controller_id IS NOT NULL
            OR NEW.last_fencing_token IS NOT NULL
            OR NEW.last_controller_id IS NOT NULL
            OR NEW.last_failure_attempt_no IS NOT NULL
            OR NEW.live_attestation_id IS NOT NULL
            OR NEW.snapshot -> 'controller_lease' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'last_fencing_token' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'preflight' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'drain' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'activation' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'panel_certificate' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'gateway_ready' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'live' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'last_live_recovery' IS DISTINCT FROM 'null'::JSONB
            OR NEW.snapshot -> 'last_runtime_failure' IS DISTINCT FROM 'null'::JSONB
        THEN
            RAISE EXCEPTION 'pending runtime attempt must be pristine'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.last_fencing_token IS NULL OR NEW.last_controller_id IS NULL THEN
        RAISE EXCEPTION 'started runtime attempt requires fenced controller history'
            USING ERRCODE = '23514';
    END IF;

    IF pending_condition IN ('retryable', 'blocked')
        AND NEW.controller_id IS NULL
        AND NEW.last_failure_attempt_no IS DISTINCT FROM NEW.convergence_attempt_no
    THEN
        RAISE EXCEPTION 'finished runtime attempt must bind its failure'
            USING ERRCODE = '23514';
    END IF;

    IF TG_OP = 'INSERT' THEN
        IF NEW.convergence_attempt_no <> 0
            OR NEW.last_failure_attempt_no IS NOT NULL
            OR NEW.last_controller_id IS NOT NULL
        THEN
            RAISE EXCEPTION 'new runtime deployment must begin before its first attempt'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF NEW.last_fencing_token IS NOT DISTINCT FROM OLD.last_fencing_token
            AND NEW.last_controller_id IS DISTINCT FROM OLD.last_controller_id
        THEN
            RAISE EXCEPTION 'runtime controller identity cannot change without a new fence'
                USING ERRCODE = '23514';
        END IF;

        IF NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token
            AND (
                NEW.controller_id IS NULL
                OR NEW.last_controller_id IS DISTINCT FROM NEW.controller_id
            )
        THEN
            RAISE EXCEPTION 'runtime fencing transition lacks its controller identity'
                USING ERRCODE = '23514';
        END IF;

        lease_claimed := NEW.controller_id IS NOT NULL AND (
            OLD.controller_id IS NULL
            OR NEW.controller_id IS DISTINCT FROM OLD.controller_id
            OR NEW.controller_fencing_token IS DISTINCT FROM OLD.controller_fencing_token
            OR NEW.controller_acquired_at IS DISTINCT FROM OLD.controller_acquired_at
            OR NEW.controller_lease_expires_at IS DISTINCT FROM OLD.controller_lease_expires_at
        );
        failure_changed := NEW.snapshot -> 'last_runtime_failure'
            IS DISTINCT FROM OLD.snapshot -> 'last_runtime_failure';
        operator_recovery := lease_claimed
            AND OLD.snapshot #>> '{phase,condition,condition}' = 'blocked'
            AND NEW.snapshot #>> '{phase,condition,condition}' = 'ready'
            AND OLD.controller_id IS NULL
            AND OLD.last_failure_attempt_no = OLD.convergence_attempt_no
            AND NEW.last_failure_attempt_no = OLD.last_failure_attempt_no
            AND NOT failure_changed;

        IF NEW.convergence_attempt_no NOT IN (
            OLD.convergence_attempt_no,
            OLD.convergence_attempt_no + 1
        ) THEN
            RAISE EXCEPTION 'runtime convergence attempt must advance once'
                USING ERRCODE = '23514';
        END IF;

        IF NEW.convergence_attempt_no = OLD.convergence_attempt_no + 1
            AND NOT lease_claimed
        THEN
            RAISE EXCEPTION 'runtime convergence attempt requires a fresh execution claim'
                USING ERRCODE = '23514';
        END IF;

        IF lease_claimed AND (
            NEW.controller_acquired_at IS DISTINCT FROM mutation_clock
            OR NEW.controller_fencing_token IS NULL
            OR NEW.last_fencing_token IS DISTINCT FROM NEW.controller_fencing_token
            OR NEW.last_fencing_token <= COALESCE(OLD.last_fencing_token, 0)
            OR NEW.live_attestation_id IS DISTINCT FROM OLD.live_attestation_id
            OR (
                NOT operator_recovery
                AND NEW.snapshot - ARRAY[
                    'revision',
                    'controller_lease',
                    'last_fencing_token'
                ]::TEXT[] IS DISTINCT FROM OLD.snapshot - ARRAY[
                    'revision',
                    'controller_lease',
                    'last_fencing_token'
                ]::TEXT[]
            )
            OR (
                operator_recovery
                AND NEW.snapshot - ARRAY[
                    'revision',
                    'controller_lease',
                    'last_fencing_token',
                    'phase'
                ]::TEXT[] IS DISTINCT FROM OLD.snapshot - ARRAY[
                    'revision',
                    'controller_lease',
                    'last_fencing_token',
                    'phase'
                ]::TEXT[]
            )
            OR (
                OLD.controller_id IS NOT NULL
                AND OLD.controller_lease_expires_at > mutation_clock
                AND (
                    operator_recovery
                    OR NEW.convergence_attempt_no <> OLD.convergence_attempt_no
                    OR NEW.controller_id IS DISTINCT FROM OLD.controller_id
                    OR NEW.controller_lease_expires_at <= OLD.controller_lease_expires_at
                )
            )
            OR (
                (
                    OLD.controller_id IS NULL
                    OR OLD.controller_lease_expires_at <= mutation_clock
                )
                AND NEW.convergence_attempt_no <> OLD.convergence_attempt_no + 1
            )
            OR (
                NOT operator_recovery
                AND OLD.snapshot #>> '{phase,condition,condition}' = 'blocked'
            )
            OR (
                NOT operator_recovery
                AND OLD.snapshot #>> '{phase,condition,condition}' = 'retryable'
                AND OLD.next_retry_at > mutation_clock
            )
        ) THEN
            RAISE EXCEPTION 'runtime execution claim transition is invalid'
                USING ERRCODE = '23514';
        END IF;

        IF NOT lease_claimed
            AND NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token
        THEN
            RAISE EXCEPTION 'runtime fencing history requires an execution claim'
                USING ERRCODE = '23514';
        END IF;

        IF OLD.controller_id IS NOT NULL
            AND NEW.controller_id IS NULL
            AND (
                OLD.controller_lease_expires_at <= mutation_clock
                OR NOT (
                    failure_changed
                    OR NEW.phase = 'live'
                    OR NEW.phase IN ('superseded', 'cancelled')
                )
            )
        THEN
            RAISE EXCEPTION 'runtime controller lease release is invalid'
                USING ERRCODE = '23514';
        END IF;

        IF failure_changed
            AND NEW.last_failure_attempt_no IS NOT DISTINCT FROM OLD.last_failure_attempt_no
        THEN
            RAISE EXCEPTION 'runtime failure evidence is immutable within an attempt'
                USING ERRCODE = '23514';
        END IF;

        IF NEW.last_failure_attempt_no IS DISTINCT FROM OLD.last_failure_attempt_no
            AND (
                NEW.last_failure_attempt_no IS NULL
                OR NEW.last_failure_attempt_no IS DISTINCT FROM NEW.convergence_attempt_no
                OR NEW.controller_id IS NOT NULL
                OR pending_condition NOT IN ('retryable', 'blocked')
                OR NEW.snapshot -> 'last_runtime_failure'
                    IS NOT DISTINCT FROM OLD.snapshot -> 'last_runtime_failure'
                OR OLD.controller_id IS NULL
                OR OLD.controller_lease_expires_at <= mutation_clock
                OR (
                    OLD.last_failure_attempt_no IS NOT NULL
                    AND NEW.last_failure_attempt_no <= OLD.last_failure_attempt_no
                )
            )
        THEN
            RAISE EXCEPTION 'runtime failure attempt transition is invalid'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    RETURN NEW;
END;
$function$;

DO $postflight$
DECLARE
    common_owner OID;
    column_count BIGINT;
    invalid_column_count BIGINT;
    constraint_count BIGINT;
    invalid_constraint_count BIGINT;
    constraint_definition TEXT;
    duration_column_count BIGINT;
    invalid_duration_column_count BIGINT;
    duration_constraint_count BIGINT;
    invalid_duration_constraint_count BIGINT;
    duration_constraint_definition TEXT;
    function_count BIGINT;
    invalid_function_count BIGINT;
    trigger_count BIGINT;
    invalid_trigger_count BIGINT;
    function_definition TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*), pg_catalog.count(*) FILTER (
        WHERE attribute.atttypid <> pg_catalog.to_regtype('text')
            OR attribute.attnotnull
            OR attribute.atthasdef
            OR attribute.attmissingval IS NOT NULL
    )
    INTO column_count, invalid_column_count
    FROM pg_catalog.pg_attribute AS attribute
    WHERE attribute.attrelid = pg_catalog.to_regclass('public.runtime_deployments')
        AND attribute.attname = 'last_controller_id'
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    SELECT pg_catalog.count(*), pg_catalog.count(*) FILTER (
        WHERE constraint_row.contype <> 'c'
            OR NOT constraint_row.convalidated
            OR constraint_row.condeferrable
            OR constraint_row.condeferred
    )
    INTO constraint_count, invalid_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass('public.runtime_deployments')
        AND constraint_row.conname = 'runtime_deployments_last_controller_valid';

    SELECT pg_catalog.count(*), pg_catalog.count(*) FILTER (
        WHERE attribute.atttypid <> pg_catalog.to_regtype('bigint')
            OR NOT attribute.attnotnull
            OR attribute.atthasdef
            OR attribute.attmissingval IS NOT NULL
    )
    INTO duration_column_count, invalid_duration_column_count
    FROM pg_catalog.pg_attribute AS attribute
    WHERE attribute.attrelid = pg_catalog.to_regclass('public.runtime_attestations')
        AND attribute.attname = 'serving_lease_duration_nanos'
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    SELECT pg_catalog.count(*), pg_catalog.count(*) FILTER (
        WHERE constraint_row.contype <> 'c'
            OR NOT constraint_row.convalidated
            OR constraint_row.condeferrable
            OR constraint_row.condeferred
    )
    INTO duration_constraint_count, invalid_duration_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass('public.runtime_attestations')
        AND constraint_row.conname = 'runtime_attestations_serving_lease_duration_valid';

    SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid, FALSE)
    INTO duration_constraint_definition
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass('public.runtime_attestations')
        AND constraint_row.conname = 'runtime_attestations_serving_lease_duration_valid';

    SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid, FALSE)
    INTO constraint_definition
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass('public.runtime_deployments')
        AND constraint_row.conname = 'runtime_deployments_last_controller_valid';

    SELECT pg_catalog.count(*), pg_catalog.count(*) FILTER (
        WHERE function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR NOT function_row.prosecdef
            OR function_row.proretset
            OR function_row.proconfig
                IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
            OR language_row.lanname <> 'plpgsql'
            OR pg_catalog.pg_get_function_identity_arguments(function_row.oid) <> ''
            OR pg_catalog.pg_get_function_result(function_row.oid) <> 'trigger'
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
    )
    INTO function_count, invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_convergence_attempt_projection()'
    );

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO function_definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_convergence_attempt_projection()'
    );

    SELECT pg_catalog.count(*), pg_catalog.count(*) FILTER (
        WHERE trigger_row.tgfoid <> pg_catalog.to_regprocedure(
                'public.validate_runtime_convergence_attempt_projection()'
            )
            OR pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE)
                <> 'CREATE TRIGGER runtime_deployments_validate_convergence_attempt BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_convergence_attempt_projection()'
            OR trigger_row.tgenabled <> 'O'
            OR trigger_row.tgisinternal
            OR trigger_row.tgparentid <> 0
            OR trigger_row.tgconstraint <> 0
            OR trigger_row.tgdeferrable
            OR trigger_row.tginitdeferred
            OR pg_catalog.cardinality(trigger_row.tgattr) <> 0
            OR trigger_row.tgnargs <> 0
            OR pg_catalog.octet_length(trigger_row.tgargs) <> 0
    )
    INTO trigger_count, invalid_trigger_count
    FROM pg_catalog.pg_trigger AS trigger_row
    WHERE trigger_row.tgrelid = pg_catalog.to_regclass('public.runtime_deployments')
        AND trigger_row.tgname = 'runtime_deployments_validate_convergence_attempt';

    IF common_owner IS NULL
        OR column_count <> 1
        OR invalid_column_count <> 0
        OR constraint_count <> 1
        OR invalid_constraint_count <> 0
        OR constraint_definition <> 'CHECK (((((last_fencing_token IS NULL) AND (last_controller_id IS NULL)) OR ((last_fencing_token IS NOT NULL) AND (last_controller_id IS NOT NULL) AND (last_controller_id ~ ''^[A-Za-z0-9_.:-]{1,128}$''::text))) AND ((controller_id IS NULL) OR (NOT (last_controller_id IS DISTINCT FROM controller_id)))))'
        OR duration_column_count <> 1
        OR invalid_duration_column_count <> 0
        OR duration_constraint_count <> 1
        OR invalid_duration_constraint_count <> 0
        OR duration_constraint_definition <> 'CHECK (((serving_lease_duration_nanos >= 1) AND (serving_lease_duration_nanos <= ''9223372036854775807''::bigint)))'
        OR function_count <> 1
        OR invalid_function_count <> 0
        OR trigger_count <> 1
        OR invalid_trigger_count <> 0
        OR function_definition NOT LIKE '%runtime controller identity cannot change without a new fence%'
        OR function_definition NOT LIKE '%runtime fencing transition lacks its controller identity%'
        OR function_definition NOT LIKE '%runtime execution claim transition is invalid%'
    THEN
        RAISE EXCEPTION 'runtime last controller installation is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
