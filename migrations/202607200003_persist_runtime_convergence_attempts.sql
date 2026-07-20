SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.runtime_deployments, public.runtime_attestations, public.runtime_serving_leases
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    relation_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    collision_count BIGINT;
    required_trigger_count BIGINT;
    function_contract_count BIGINT;
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
        RAISE EXCEPTION 'runtime convergence attempt relations require their common owner'
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
    WHERE defaults.defaclnamespace IN (
        0,
        pg_catalog.to_regnamespace('public')
    )
        AND privilege.grantee <> defaults.defaclrole;

    IF unsafe_schema_create_count <> 0 OR unsafe_default_count <> 0 THEN
        RAISE EXCEPTION 'runtime convergence attempt schema trust is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_attribute AS attribute
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = attribute.attrelid
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
        AND relation.relname IN ('runtime_deployments', 'runtime_attestations')
        AND attribute.attname IN ('convergence_attempt_no', 'last_failure_attempt_no')
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    IF collision_count <> 0
        OR pg_catalog.to_regprocedure(
            'public.validate_runtime_convergence_attempt_projection()'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'public.validate_runtime_attestation_attempt_projection()'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION 'runtime convergence attempt schema collides with an existing object'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO required_trigger_count
    FROM (
        VALUES
            ('public.runtime_deployments', 'public.guard_runtime_ruleset_artifact_transition()', 'CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()'),
            ('public.runtime_deployments', 'public.enforce_runtime_deployment_policy_shadow()', 'CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()'),
            ('public.runtime_deployments', 'public.validate_runtime_deployment_projection()', 'CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()'),
            ('public.runtime_deployments', 'public.reject_runtime_deployment_delete()', 'CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()'),
            ('public.runtime_attestations', 'public.validate_runtime_attestation_projection()', 'CREATE TRIGGER runtime_attestations_validate_projection BEFORE INSERT ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_attestation_projection()'),
            ('public.runtime_attestations', 'public.reject_immutable_product_row()', 'CREATE TRIGGER runtime_attestations_reject_mutation BEFORE DELETE OR UPDATE ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_row()'),
            ('public.runtime_serving_leases', 'public.validate_runtime_serving_lease_transition()', 'CREATE TRIGGER runtime_serving_leases_validate_transition BEFORE INSERT OR UPDATE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_serving_lease_transition()'),
            ('public.runtime_serving_leases', 'public.reject_runtime_serving_lease_delete()', 'CREATE TRIGGER runtime_serving_leases_reject_delete BEFORE DELETE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_serving_lease_delete()')
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

    IF required_trigger_count <> 8 OR (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_trigger AS trigger_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = trigger_row.tgrelid
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public'
            AND relation.relname IN (
                'runtime_deployments',
                'runtime_attestations',
                'runtime_serving_leases'
            )
            AND NOT trigger_row.tgisinternal
    ) <> 8 THEN
        RAISE EXCEPTION 'runtime convergence attempt protection is incomplete'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_contract_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IN (
            pg_catalog.to_regprocedure('public.validate_runtime_deployment_projection()'),
            pg_catalog.to_regprocedure('public.validate_runtime_attestation_projection()')
        )
        AND function_row.proowner = common_owner
        AND function_row.prokind = 'f'
        AND function_row.provolatile = 'v'
        AND NOT function_row.proisstrict
        AND function_row.proparallel = 'u'
        AND function_row.prosecdef
        AND NOT function_row.proretset
        AND function_row.prorows = 0
        AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
        AND language_row.lanname = 'plpgsql'
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );

    IF function_contract_count <> 2 THEN
        RAISE EXCEPTION 'runtime convergence attempt function trust is invalid'
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
        RAISE EXCEPTION 'legacy runtime execution history cannot be inferred safely'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_deployments
ADD COLUMN convergence_attempt_no BIGINT NOT NULL DEFAULT 0,
ADD COLUMN last_failure_attempt_no BIGINT;

ALTER TABLE public.runtime_deployments
ADD CONSTRAINT runtime_deployments_convergence_attempt_valid CHECK (
    convergence_attempt_no BETWEEN 0 AND 4294967295
    AND (
        last_failure_attempt_no IS NULL
        OR last_failure_attempt_no BETWEEN 1 AND convergence_attempt_no
    )
) NOT VALID;

ALTER TABLE public.runtime_deployments
VALIDATE CONSTRAINT runtime_deployments_convergence_attempt_valid;

ALTER TABLE public.runtime_attestations
ADD COLUMN convergence_attempt_no BIGINT NOT NULL;

ALTER TABLE public.runtime_attestations
ADD CONSTRAINT runtime_attestations_convergence_attempt_valid CHECK (
    convergence_attempt_no BETWEEN 1 AND 4294967295
) NOT VALID;

ALTER TABLE public.runtime_attestations
ADD CONSTRAINT runtime_attestations_deployment_attempt_unique UNIQUE (
    deployment_id,
    convergence_attempt_no
);

ALTER TABLE public.runtime_attestations
VALIDATE CONSTRAINT runtime_attestations_convergence_attempt_valid;

CREATE FUNCTION public.validate_runtime_convergence_attempt_projection()
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
    ELSIF NEW.last_fencing_token IS NULL THEN
        RAISE EXCEPTION 'started runtime attempt requires fenced execution history'
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
        THEN
            RAISE EXCEPTION 'new runtime deployment must begin before its first attempt'
                USING ERRCODE = '23514';
        END IF;
    ELSE
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

CREATE TRIGGER runtime_deployments_validate_convergence_attempt
BEFORE INSERT OR UPDATE ON public.runtime_deployments
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_convergence_attempt_projection();

CREATE FUNCTION public.validate_runtime_attestation_attempt_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    deployment_attempt BIGINT;
BEGIN
    SELECT deployment.convergence_attempt_no
    INTO deployment_attempt
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = NEW.tenant_id
        AND deployment.installation_id = NEW.installation_id
        AND deployment.deployment_id = NEW.deployment_id
    FOR SHARE;

    IF deployment_attempt IS NULL
        OR deployment_attempt = 0
        OR NEW.convergence_attempt_no IS DISTINCT FROM deployment_attempt
    THEN
        RAISE EXCEPTION 'runtime attestation differs from its durable convergence attempt'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_attestations_validate_convergence_attempt
BEFORE INSERT ON public.runtime_attestations
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_attestation_attempt_projection();

REVOKE ALL ON FUNCTION public.validate_runtime_convergence_attempt_projection() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.validate_runtime_attestation_attempt_projection() FROM PUBLIC;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
