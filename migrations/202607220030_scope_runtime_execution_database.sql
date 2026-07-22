SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.runtime_deployments,
    public.runtime_attestations,
    public.runtime_serving_leases,
    public.activation_requests,
    public.authoring_promotions,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 11
        OR ordinary_count <> 11
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_relation_drift';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_owner_drift';
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
        AND privilege.grantee <> namespace.nspowner;

    IF unsafe_schema_create_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_schema_authority_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_execution_schema_manifest_v1',
            'starring_runtime_execution_database_readiness_v1',
            'starring_runtime_execution_database_identity_v1',
            'starring_runtime_execution_claim_next_v1',
            'starring_runtime_execution_renew_v1',
            'starring_runtime_execution_mutate_v1',
            'starring_runtime_execution_certify_prepare_v1',
            'starring_runtime_execution_certify_commit_v1',
            'starring_runtime_execution_recover_stale_live_v1',
            'validate_runtime_execution_mutation_marker_transition',
            'reject_runtime_execution_mutation_marker_delete'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_function_drift';
    END IF;

    IF pg_catalog.to_regclass(
            'public.runtime_execution_mutation_markers'
        ) IS NOT NULL
        OR pg_catalog.to_regclass(
            'public.runtime_deployments_active_controller_index'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_relation_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_execution_mutation_markers (
    deployment_id TEXT PRIMARY KEY,
    mutation_revision BIGINT NOT NULL,
    mutation_kind TEXT NOT NULL,
    mutation_payload JSONB NOT NULL,
    CONSTRAINT runtime_execution_mutation_markers_deployment_fk
        FOREIGN KEY (deployment_id)
        REFERENCES public.runtime_deployments (deployment_id)
        ON DELETE RESTRICT,
    CONSTRAINT runtime_execution_mutation_markers_revision_check CHECK (
        mutation_revision BETWEEN 2 AND 9223372036854775807
    ),
    CONSTRAINT runtime_execution_mutation_markers_kind_check CHECK (
        mutation_kind IN (
            'accept_preflight',
            'request_drain',
            'accept_drain',
            'begin_activation',
            'accept_activation',
            'record_retryable_failure',
            'record_blocked_failure',
            'resume_runtime_pending',
            'begin_panel_reconciliation',
            'accept_panel_certificate',
            'supersede',
            'cancel'
        )
    ),
    CONSTRAINT runtime_execution_mutation_markers_payload_check CHECK (
        pg_catalog.jsonb_typeof(mutation_payload) = 'object'
        AND pg_catalog.octet_length(mutation_payload::TEXT) <= 262144
    )
);

CREATE FUNCTION public.validate_runtime_execution_mutation_marker_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    deployment_revision BIGINT;
BEGIN
    IF TG_OP = 'UPDATE'
        AND (
            NEW.deployment_id IS DISTINCT FROM OLD.deployment_id
            OR NEW.mutation_revision <= OLD.mutation_revision
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_execution_mutation_marker_transition_invalid';
    END IF;

    SELECT deployment.revision
    INTO deployment_revision
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id = NEW.deployment_id
    FOR UPDATE;

    IF NOT FOUND
        OR NEW.mutation_revision IS DISTINCT FROM deployment_revision
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_execution_mutation_marker_parent_invalid';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_execution_mutation_markers_validate_transition
BEFORE INSERT OR UPDATE ON public.runtime_execution_mutation_markers
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_execution_mutation_marker_transition();

CREATE FUNCTION public.reject_runtime_execution_mutation_marker_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_execution_mutation_marker_delete_rejected';
END;
$function$;

CREATE TRIGGER runtime_execution_mutation_markers_reject_delete
BEFORE DELETE ON public.runtime_execution_mutation_markers
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_execution_mutation_marker_delete();

REVOKE ALL ON FUNCTION
    public.validate_runtime_execution_mutation_marker_transition()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_execution_mutation_marker_delete()
FROM PUBLIC;

CREATE INDEX runtime_deployments_active_controller_index
ON public.runtime_deployments (
    controller_id,
    controller_lease_expires_at,
    controller_acquired_at,
    deployment_id
)
WHERE controller_id IS NOT NULL;

CREATE FUNCTION public.starring_runtime_execution_database_identity_v1()
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

CREATE FUNCTION public.starring_runtime_execution_claim_next_v1(
    expected_controller_id TEXT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    previous_snapshot JSONB,
    snapshot JSONB,
    controller_id TEXT,
    fencing_token BIGINT,
    previous_convergence_attempt_no BIGINT,
    convergence_attempt_no BIGINT,
    acquired_at TIMESTAMPTZ,
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
    deployment_row public.runtime_deployments%ROWTYPE;
    previous_snapshot_value JSONB;
    next_snapshot JSONB;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    replay_lookup_clock TIMESTAMPTZ;
    replay_validation_clock TIMESTAMPTZ;
    requested_duration INTERVAL;
    next_revision BIGINT;
    next_fencing_token BIGINT;
    next_attempt BIGINT;
    next_expiry TIMESTAMPTZ;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 600000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_claim_input_invalid';
    END IF;

    requested_duration :=
        requested_lease_milliseconds * INTERVAL '1 millisecond';

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-execution-controller-v1:',
                expected_controller_id
            ),
            0
        )
    );
    replay_lookup_clock := pg_catalog.clock_timestamp();

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.controller_id = expected_controller_id
        AND deployment.controller_lease_expires_at
            > replay_lookup_clock
        AND deployment.phase NOT IN ('live', 'superseded', 'cancelled')
    ORDER BY deployment.controller_acquired_at, deployment.deployment_id
    LIMIT 1
    FOR UPDATE;

    IF FOUND THEN
        replay_validation_clock := GREATEST(
            pg_catalog.clock_timestamp(),
            replay_lookup_clock
        );
        IF EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS duplicate
            WHERE duplicate.controller_id = expected_controller_id
                AND duplicate.controller_lease_expires_at
                    > replay_validation_clock
                AND duplicate.phase NOT IN ('live', 'superseded', 'cancelled')
                AND duplicate.deployment_id <> deployment_row.deployment_id
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_claim_controller_ambiguous';
        END IF;

        mutation_clock := replay_validation_clock;
        IF deployment_row.controller_lease_expires_at > mutation_clock THEN
            IF deployment_row.last_controller_id
                    IS DISTINCT FROM expected_controller_id
                OR deployment_row.controller_fencing_token IS NULL
                OR deployment_row.controller_fencing_token
                    IS DISTINCT FROM deployment_row.last_fencing_token
                OR deployment_row.controller_acquired_at IS NULL
                OR deployment_row.controller_lease_expires_at
                    - deployment_row.controller_acquired_at
                    IS DISTINCT FROM requested_duration
                OR deployment_row.convergence_attempt_no
                    NOT BETWEEN 1 AND 4294967295
                OR deployment_row.snapshot #>> '{controller_lease,controller_id}'
                    IS DISTINCT FROM expected_controller_id
                OR deployment_row.snapshot #>> '{controller_lease,fencing_token}'
                    IS DISTINCT FROM deployment_row.controller_fencing_token::TEXT
                OR (deployment_row.snapshot
                        #>> '{controller_lease,acquired_at}')::TIMESTAMPTZ
                    IS DISTINCT FROM deployment_row.controller_acquired_at
                OR (deployment_row.snapshot
                        #>> '{controller_lease,expires_at}')::TIMESTAMPTZ
                    IS DISTINCT FROM deployment_row.controller_lease_expires_at
                OR deployment_row.snapshot ->> 'revision'
                    IS DISTINCT FROM deployment_row.revision::TEXT
                OR deployment_row.snapshot ->> 'last_fencing_token'
                    IS DISTINCT FROM deployment_row.last_fencing_token::TEXT
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX004',
                    MESSAGE = 'runtime_execution_claim_replay_mismatch';
            END IF;

            authority_outcome := public.starring_runtime_lock_current_authority(
                deployment_row.activation_request_id,
                deployment_row.promotion_id,
                deployment_row.tenant_id,
                deployment_row.installation_id,
                deployment_row.installation_authority_revision,
                deployment_row.guild_id,
                deployment_row.ruleset_key,
                deployment_row.target_version,
                deployment_row.target_content_hash,
                deployment_row.binding_revision,
                deployment_row.binding_fingerprint
            );
            IF authority_outcome = 'active_mismatch' THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX006',
                    MESSAGE = 'runtime_execution_claim_target_superseded';
            ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX003',
                    MESSAGE = 'runtime_execution_claim_authority_changed';
            END IF;

            replay_validation_clock := GREATEST(
                pg_catalog.clock_timestamp(),
                replay_validation_clock
            );
            IF deployment_row.controller_lease_expires_at
                > replay_validation_clock
            THEN
                outcome_name := 'replayed';
                previous_snapshot := deployment_row.snapshot;
                snapshot := deployment_row.snapshot;
                controller_id := deployment_row.controller_id;
                fencing_token := deployment_row.controller_fencing_token;
                previous_convergence_attempt_no :=
                    deployment_row.convergence_attempt_no - 1;
                convergence_attempt_no :=
                    deployment_row.convergence_attempt_no;
                acquired_at := deployment_row.controller_acquired_at;
                expires_at := deployment_row.controller_lease_expires_at;
                RETURN NEXT;
                RETURN;
            END IF;
        END IF;
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    JOIN public.activation_requests AS activation
        ON activation.id = deployment.activation_request_id
        AND activation.state = 'applied'
        AND activation.authority_kind = 'product_authoring'
        AND activation.link_state_name = 'linked'
        AND activation.promotion_id = deployment.promotion_id
    JOIN public.authoring_promotions AS promotion
        ON promotion.id = deployment.promotion_id
        AND promotion.stage = 'activation_pending'
        AND promotion.tenant_id = deployment.tenant_id
    JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = deployment.tenant_id
        AND tenant.lifecycle_state = 'active'
    JOIN public.automation_installations AS installation
        ON installation.tenant_id = deployment.tenant_id
        AND installation.installation_id = deployment.installation_id
        AND installation.lifecycle_state = 'active'
    JOIN public.automation_installation_authority_versions
        AS historical_authority
        ON historical_authority.tenant_id = installation.tenant_id
        AND historical_authority.installation_id
            = installation.installation_id
        AND historical_authority.revision
            = deployment.installation_authority_revision
        AND historical_authority.binding_revision
            = deployment.binding_revision
        AND historical_authority.binding_fingerprint
            = deployment.binding_fingerprint
    JOIN public.automation_installation_authority_versions
        AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id
            = installation.installation_id
        AND current_authority.revision
            = installation.current_authority_revision
        AND current_authority.binding_revision
            = deployment.binding_revision
        AND current_authority.binding_fingerprint
            = deployment.binding_fingerprint
        AND current_authority.resource_bindings
            IS NOT DISTINCT FROM historical_authority.resource_bindings
    JOIN public.automation_ruleset_activations AS active
        ON active.guild_id = deployment.guild_id
        AND active.ruleset_key = deployment.ruleset_key
        AND active.active_version = deployment.target_version
    JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
        AND version.content_hash = deployment.target_content_hash
        AND version.canonical_content_hash = version.content_hash
        AND version.schema_version = 1
    WHERE deployment.phase NOT IN ('live', 'superseded', 'cancelled')
        AND deployment.blocked_at IS NULL
        AND (
            deployment.next_retry_at IS NULL
            OR deployment.next_retry_at <= pg_catalog.clock_timestamp()
        )
        AND (
            deployment.controller_lease_expires_at IS NULL
            OR deployment.controller_lease_expires_at
                <= pg_catalog.clock_timestamp()
        )
        AND promotion.record #>> '{intent,authority,tenant_id}'
            = deployment.tenant_id
        AND promotion.record #>> '{intent,authority,installation_id}'
            = deployment.installation_id
        AND promotion.record #>> '{intent,authority,guild_id}'
            = deployment.guild_id
        AND promotion.record #>> '{intent,authority,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record #>> '{intent,authority,binding_revision}'
            = deployment.binding_revision::TEXT
        AND promotion.record #>> '{intent,evidence,context_fingerprint}'
            = deployment.binding_fingerprint
        AND promotion.record #>> '{stage,activation,request_id}'
            = deployment.activation_request_id
        AND promotion.record #>> '{stage,activation,target,guild_id}'
            = deployment.guild_id
        AND promotion.record #>> '{stage,activation,target,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record #>> '{stage,activation,target,version}'
            = deployment.target_version::TEXT
        AND promotion.record #>> '{stage,activation,target,content_hash}'
            = deployment.target_content_hash
    ORDER BY
        COALESCE(deployment.next_retry_at, deployment.requested_at),
        deployment.requested_at,
        deployment.deployment_id
    LIMIT 1
    FOR UPDATE OF deployment SKIP LOCKED;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );

    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_claim_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_claim_authority_changed';
    END IF;

    IF deployment_row.revision = 9223372036854775807
        OR COALESCE(deployment_row.last_fencing_token, 0)
            = 9223372036854775807
        OR deployment_row.convergence_attempt_no
            NOT BETWEEN 0 AND 4294967294
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM deployment_row.revision::TEXT
        OR deployment_row.snapshot ->> 'runtime_generation'
            IS DISTINCT FROM deployment_row.runtime_generation::TEXT
        OR deployment_row.snapshot #>> '{identity,deployment_id}'
            IS DISTINCT FROM deployment_row.deployment_id
        OR deployment_row.snapshot #>> '{identity,tenant_id}'
            IS DISTINCT FROM deployment_row.tenant_id
        OR deployment_row.snapshot #>> '{identity,installation_id}'
            IS DISTINCT FROM deployment_row.installation_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_claim_state_invalid';
    END IF;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF deployment_row.next_retry_at IS NOT NULL
        AND deployment_row.next_retry_at > mutation_clock
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_execution_claim_retry_not_ready';
    END IF;
    IF deployment_row.phase = 'runtime_pending'
        AND deployment_row.snapshot #>> '{phase,condition,condition}'
            = 'blocked'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_claim_operator_action_required';
    END IF;

    previous_snapshot_value := deployment_row.snapshot;
    next_revision := deployment_row.revision + 1;
    next_fencing_token :=
        COALESCE(deployment_row.last_fencing_token, 0) + 1;
    next_attempt := deployment_row.convergence_attempt_no + 1;
    next_expiry := mutation_clock + requested_duration;

    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{controller_lease}',
        pg_catalog.jsonb_build_object(
            'controller_id', expected_controller_id,
            'fencing_token', next_fencing_token,
            'acquired_at', mutation_clock,
            'expires_at', next_expiry
        ),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{last_fencing_token}',
        pg_catalog.to_jsonb(next_fencing_token),
        FALSE
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        controller_id = expected_controller_id,
        controller_fencing_token = next_fencing_token,
        controller_acquired_at = mutation_clock,
        controller_lease_expires_at = next_expiry,
        last_fencing_token = next_fencing_token,
        last_controller_id = expected_controller_id,
        convergence_attempt_no = next_attempt,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.deployment_id = deployment_row.deployment_id
        AND deployment.revision = deployment_row.revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_claim_ownership_lost';
    END IF;

    outcome_name := 'applied';
    previous_snapshot := previous_snapshot_value;
    snapshot := next_snapshot;
    controller_id := expected_controller_id;
    fencing_token := next_fencing_token;
    previous_convergence_attempt_no :=
        deployment_row.convergence_attempt_no;
    convergence_attempt_no := next_attempt;
    acquired_at := mutation_clock;
    expires_at := next_expiry;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_execution_renew_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    previous_snapshot JSONB,
    snapshot JSONB,
    controller_id TEXT,
    fencing_token BIGINT,
    convergence_attempt_no BIGINT,
    acquired_at TIMESTAMPTZ,
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
    deployment_row public.runtime_deployments%ROWTYPE;
    previous_snapshot_value JSONB;
    next_snapshot JSONB;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    requested_duration INTERVAL;
    next_revision BIGINT;
    next_fencing_token BIGINT;
    next_expiry TIMESTAMPTZ;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 600000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_renew_input_invalid';
    END IF;

    requested_duration :=
        requested_lease_milliseconds * INTERVAL '1 millisecond';

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_renew_ownership_lost';
    END IF;

    IF deployment_row.revision = expected_deployment_revision + 1 THEN
        mutation_clock := pg_catalog.clock_timestamp();
        IF deployment_row.snapshot ->> 'revision'
                IS DISTINCT FROM deployment_row.revision::TEXT
            OR deployment_row.snapshot ->> 'runtime_generation'
                IS DISTINCT FROM deployment_row.runtime_generation::TEXT
            OR deployment_row.snapshot #>> '{phase,phase}'
                IS DISTINCT FROM deployment_row.phase
            OR (deployment_row.last_fencing_token IS NULL)
                <> (deployment_row.last_controller_id IS NULL)
            OR deployment_row.snapshot ->> 'last_fencing_token'
                IS DISTINCT FROM deployment_row.last_fencing_token::TEXT
            OR (
                deployment_row.controller_id IS NULL
                AND (
                    deployment_row.controller_fencing_token IS NOT NULL
                    OR deployment_row.controller_acquired_at IS NOT NULL
                    OR deployment_row.controller_lease_expires_at IS NOT NULL
                    OR deployment_row.snapshot -> 'controller_lease'
                        IS DISTINCT FROM 'null'::JSONB
                )
            )
            OR (
                deployment_row.controller_id IS NOT NULL
                AND (
                    deployment_row.controller_fencing_token IS NULL
                    OR deployment_row.controller_acquired_at IS NULL
                    OR deployment_row.controller_lease_expires_at IS NULL
                    OR deployment_row.controller_acquired_at
                        >= deployment_row.controller_lease_expires_at
                    OR deployment_row.last_controller_id
                        IS DISTINCT FROM deployment_row.controller_id
                    OR deployment_row.last_fencing_token
                        IS DISTINCT FROM deployment_row.controller_fencing_token
                    OR pg_catalog.jsonb_typeof(
                        deployment_row.snapshot -> 'controller_lease'
                    ) IS DISTINCT FROM 'object'
                    OR deployment_row.snapshot
                        #>> '{controller_lease,controller_id}'
                        IS DISTINCT FROM deployment_row.controller_id
                    OR deployment_row.snapshot
                        #>> '{controller_lease,fencing_token}'
                        IS DISTINCT FROM
                            deployment_row.controller_fencing_token::TEXT
                    OR CASE
                        WHEN pg_catalog.pg_input_is_valid(
                            deployment_row.snapshot
                                #>> '{controller_lease,acquired_at}',
                            'timestamp with time zone'
                        ) THEN (
                            deployment_row.snapshot
                                #>> '{controller_lease,acquired_at}'
                        )::TIMESTAMPTZ IS DISTINCT FROM
                            deployment_row.controller_acquired_at
                        ELSE TRUE
                    END
                    OR CASE
                        WHEN pg_catalog.pg_input_is_valid(
                            deployment_row.snapshot
                                #>> '{controller_lease,expires_at}',
                            'timestamp with time zone'
                        ) THEN (
                            deployment_row.snapshot
                                #>> '{controller_lease,expires_at}'
                        )::TIMESTAMPTZ IS DISTINCT FROM
                            deployment_row.controller_lease_expires_at
                        ELSE TRUE
                    END
                )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_renew_state_invalid';
        END IF;

        IF deployment_row.runtime_generation
                IS DISTINCT FROM expected_runtime_generation
            OR deployment_row.last_controller_id
                IS DISTINCT FROM expected_controller_id
            OR deployment_row.controller_id
                IS DISTINCT FROM expected_controller_id
            OR deployment_row.controller_fencing_token
                IS DISTINCT FROM expected_controller_fencing_token + 1
            OR deployment_row.last_fencing_token
                IS DISTINCT FROM expected_controller_fencing_token + 1
            OR deployment_row.convergence_attempt_no
                IS DISTINCT FROM expected_convergence_attempt_no
            OR deployment_row.controller_lease_expires_at
                - deployment_row.controller_acquired_at
                IS DISTINCT FROM requested_duration
            OR deployment_row.controller_lease_expires_at <= mutation_clock
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX001',
                MESSAGE = 'runtime_execution_renew_ownership_lost';
        END IF;

        authority_outcome := public.starring_runtime_lock_current_authority(
            deployment_row.activation_request_id,
            deployment_row.promotion_id,
            deployment_row.tenant_id,
            deployment_row.installation_id,
            deployment_row.installation_authority_revision,
            deployment_row.guild_id,
            deployment_row.ruleset_key,
            deployment_row.target_version,
            deployment_row.target_content_hash,
            deployment_row.binding_revision,
            deployment_row.binding_fingerprint
        );
        IF authority_outcome = 'active_mismatch' THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX006',
                MESSAGE = 'runtime_execution_renew_target_superseded';
        ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_execution_renew_authority_changed';
        END IF;

        outcome_name := 'replayed';
        previous_snapshot := deployment_row.snapshot;
        snapshot := deployment_row.snapshot;
        controller_id := expected_controller_id;
        fencing_token := deployment_row.controller_fencing_token;
        convergence_attempt_no := deployment_row.convergence_attempt_no;
        acquired_at := deployment_row.controller_acquired_at;
        expires_at := deployment_row.controller_lease_expires_at;
        RETURN NEXT;
        RETURN;
    END IF;

    IF deployment_row.revision IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.controller_acquired_at IS NULL
        OR deployment_row.controller_lease_expires_at IS NULL
        OR deployment_row.phase IN ('live', 'superseded', 'cancelled')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_renew_ownership_lost';
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );
    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_renew_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_renew_authority_changed';
    END IF;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF deployment_row.controller_lease_expires_at <= mutation_clock THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_renew_lease_expired';
    END IF;

    next_expiry := mutation_clock + requested_duration;
    IF next_expiry <= deployment_row.controller_lease_expires_at THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_renew_expiry_not_extended';
    END IF;

    previous_snapshot_value := deployment_row.snapshot;
    next_revision := expected_deployment_revision + 1;
    next_fencing_token := expected_controller_fencing_token + 1;
    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{controller_lease}',
        pg_catalog.jsonb_build_object(
            'controller_id', expected_controller_id,
            'fencing_token', next_fencing_token,
            'acquired_at', mutation_clock,
            'expires_at', next_expiry
        ),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{last_fencing_token}',
        pg_catalog.to_jsonb(next_fencing_token),
        FALSE
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        controller_fencing_token = next_fencing_token,
        controller_acquired_at = mutation_clock,
        controller_lease_expires_at = next_expiry,
        last_fencing_token = next_fencing_token,
        last_controller_id = expected_controller_id,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
        AND deployment.revision = expected_deployment_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_renew_ownership_lost';
    END IF;

    outcome_name := 'applied';
    previous_snapshot := previous_snapshot_value;
    snapshot := next_snapshot;
    controller_id := expected_controller_id;
    fencing_token := next_fencing_token;
    convergence_attempt_no := expected_convergence_attempt_no;
    acquired_at := mutation_clock;
    expires_at := next_expiry;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_execution_mutate_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    mutation_kind TEXT,
    mutation_payload JSONB
)
RETURNS TABLE(
    outcome_name TEXT,
    previous_snapshot JSONB,
    snapshot JSONB,
    convergence_attempt_no BIGINT,
    mutated_at TIMESTAMPTZ
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
    deployment_row public.runtime_deployments%ROWTYPE;
    marker_row public.runtime_execution_mutation_markers%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    previous_snapshot_value JSONB;
    next_snapshot JSONB;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    next_revision BIGINT;
    next_phase TEXT;
    requested_time TIMESTAMPTZ;
    retry_duration INTERVAL;
    retry_not_before TIMESTAMPTZ;
    stable_message TEXT;
    failure_value JSONB;
    disposition_value JSONB;
    release_controller BOOLEAN := FALSE;
    records_failure BOOLEAN := FALSE;
    serving_found BOOLEAN;
    closure_boundary TIMESTAMPTZ;
    payload_key_count BIGINT;
    nested_key_count BIGINT;
    identity_key_count BIGINT;
    target_key_count BIGINT;
    replay_exact BOOLEAN := FALSE;
    reason_trim_characters CONSTANT TEXT :=
        pg_catalog.chr(9)
        || pg_catalog.chr(10)
        || pg_catalog.chr(11)
        || pg_catalog.chr(12)
        || pg_catalog.chr(13)
        || pg_catalog.chr(32)
        || pg_catalog.chr(133)
        || pg_catalog.chr(160)
        || pg_catalog.chr(5760)
        || pg_catalog.chr(8192)
        || pg_catalog.chr(8193)
        || pg_catalog.chr(8194)
        || pg_catalog.chr(8195)
        || pg_catalog.chr(8196)
        || pg_catalog.chr(8197)
        || pg_catalog.chr(8198)
        || pg_catalog.chr(8199)
        || pg_catalog.chr(8200)
        || pg_catalog.chr(8201)
        || pg_catalog.chr(8202)
        || pg_catalog.chr(8232)
        || pg_catalog.chr(8233)
        || pg_catalog.chr(8239)
        || pg_catalog.chr(8287)
        || pg_catalog.chr(12288);
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR mutation_kind NOT IN (
            'accept_preflight',
            'request_drain',
            'accept_drain',
            'begin_activation',
            'accept_activation',
            'record_retryable_failure',
            'record_blocked_failure',
            'resume_runtime_pending',
            'begin_panel_reconciliation',
            'accept_panel_certificate',
            'supersede',
            'cancel'
        )
        OR pg_catalog.jsonb_typeof(mutation_payload) <> 'object'
        OR pg_catalog.octet_length(mutation_payload::TEXT) > 262144
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_mutation_input_invalid';
    END IF;

    SELECT pg_catalog.count(*)
    INTO payload_key_count
    FROM pg_catalog.jsonb_object_keys(mutation_payload);

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_mutation_ownership_lost';
    END IF;

    IF deployment_row.revision = expected_deployment_revision + 1
        AND deployment_row.runtime_generation
            IS NOT DISTINCT FROM expected_runtime_generation
        AND deployment_row.last_fencing_token
            IS NOT DISTINCT FROM expected_controller_fencing_token
        AND deployment_row.last_controller_id
            IS NOT DISTINCT FROM expected_controller_id
        AND deployment_row.convergence_attempt_no
            IS NOT DISTINCT FROM expected_convergence_attempt_no
    THEN
        SELECT marker.*
        INTO marker_row
        FROM public.runtime_execution_mutation_markers AS marker
        WHERE marker.deployment_id = expected_deployment_id;

        IF NOT FOUND
            OR marker_row.mutation_revision
                IS DISTINCT FROM deployment_row.revision
            OR marker_row.mutation_kind IS DISTINCT FROM mutation_kind
            OR marker_row.mutation_payload IS DISTINCT FROM mutation_payload
            OR marker_row.mutation_payload::TEXT
                IS DISTINCT FROM mutation_payload::TEXT
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_mutation_replay_mismatch';
        END IF;

        replay_exact := CASE mutation_kind
            WHEN 'accept_preflight' THEN
                payload_key_count = 4
                AND deployment_row.phase = 'preflight_ready'
                AND deployment_row.snapshot -> 'preflight'
                    IS NOT DISTINCT FROM mutation_payload
                AND (deployment_row.snapshot -> 'preflight')::TEXT
                    IS NOT DISTINCT FROM mutation_payload::TEXT
            WHEN 'request_drain' THEN
                deployment_row.phase = 'drain_requested'
                AND mutation_payload = '{}'::JSONB
            WHEN 'accept_drain' THEN
                payload_key_count = 3
                AND deployment_row.phase = 'drained'
                AND deployment_row.snapshot -> 'drain'
                    IS NOT DISTINCT FROM mutation_payload
                AND (deployment_row.snapshot -> 'drain')::TEXT
                    IS NOT DISTINCT FROM mutation_payload::TEXT
            WHEN 'begin_activation' THEN
                deployment_row.phase = 'activation_applying'
                AND mutation_payload = '{}'::JSONB
            WHEN 'accept_activation' THEN
                payload_key_count = 5
                AND deployment_row.phase = 'runtime_pending'
                AND deployment_row.snapshot #>> '{phase,condition,condition}'
                    = 'ready'
                AND deployment_row.snapshot -> 'activation'
                    IS NOT DISTINCT FROM mutation_payload
                AND (deployment_row.snapshot -> 'activation')::TEXT
                    IS NOT DISTINCT FROM mutation_payload::TEXT
            WHEN 'record_retryable_failure' THEN
                payload_key_count = 5
                AND deployment_row.phase = 'runtime_pending'
                AND deployment_row.snapshot
                    #>> '{phase,condition,condition}' = 'retryable'
                AND deployment_row.snapshot
                    #>> '{phase,condition,failure,failure_id}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'failure_id'
                AND deployment_row.snapshot
                    #>> '{phase,condition,failure,kind}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'kind'
                AND deployment_row.snapshot
                    #>> '{phase,condition,failure,code}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'code'
                AND deployment_row.snapshot
                    #>> '{phase,condition,attempt}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'attempt'
                AND CASE
                    WHEN pg_catalog.pg_input_is_valid(
                        mutation_payload ->> 'retry_after_milliseconds',
                        'bigint'
                    ) THEN (
                        (deployment_row.snapshot
                            #>> '{phase,condition,retry_not_before}')::TIMESTAMPTZ
                        - (deployment_row.snapshot
                            #>> '{phase,condition,failure,recorded_at}')::TIMESTAMPTZ
                    ) = (
                        mutation_payload ->> 'retry_after_milliseconds'
                    )::BIGINT * INTERVAL '1 millisecond'
                    ELSE FALSE
                END
            WHEN 'record_blocked_failure' THEN
                payload_key_count = 3
                AND deployment_row.phase = 'runtime_pending'
                AND deployment_row.snapshot
                    #>> '{phase,condition,condition}' = 'blocked'
                AND deployment_row.snapshot
                    #>> '{phase,condition,failure,failure_id}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'failure_id'
                AND deployment_row.snapshot
                    #>> '{phase,condition,failure,kind}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'kind'
                AND deployment_row.snapshot
                    #>> '{phase,condition,failure,code}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'code'
            WHEN 'resume_runtime_pending' THEN
                deployment_row.phase = 'runtime_pending'
                AND deployment_row.snapshot
                    #>> '{phase,condition,condition}' = 'ready'
                AND mutation_payload = '{}'::JSONB
            WHEN 'begin_panel_reconciliation' THEN
                deployment_row.phase = 'reconciling_panels'
                AND mutation_payload = '{}'::JSONB
            WHEN 'accept_panel_certificate' THEN
                payload_key_count = 16
                AND deployment_row.phase = 'awaiting_gateway_ready'
                AND deployment_row.snapshot -> 'panel_certificate'
                    IS NOT DISTINCT FROM mutation_payload
                AND (deployment_row.snapshot -> 'panel_certificate')::TEXT
                    IS NOT DISTINCT FROM mutation_payload::TEXT
            WHEN 'supersede' THEN
                payload_key_count = 2
                AND pg_catalog.octet_length(
                    mutation_payload ->> 'reason'
                ) <= 1024
                AND pg_catalog.translate(
                    mutation_payload ->> 'reason',
                    reason_trim_characters,
                    ''
                ) <> ''
                AND deployment_row.phase = 'superseded'
                AND deployment_row.snapshot #> '{phase,by}'
                    IS NOT DISTINCT FROM mutation_payload -> 'by'
                AND (deployment_row.snapshot #> '{phase,by}')::TEXT
                    IS NOT DISTINCT FROM (mutation_payload -> 'by')::TEXT
                AND deployment_row.snapshot #>> '{phase,reason}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'reason'
            WHEN 'cancel' THEN
                payload_key_count = 1
                AND pg_catalog.octet_length(
                    mutation_payload ->> 'reason'
                ) <= 1024
                AND pg_catalog.translate(
                    mutation_payload ->> 'reason',
                    reason_trim_characters,
                    ''
                ) <> ''
                AND deployment_row.phase = 'cancelled'
                AND deployment_row.snapshot #>> '{phase,reason}'
                    IS NOT DISTINCT FROM mutation_payload ->> 'reason'
            ELSE FALSE
        END;

        IF replay_exact THEN
            mutation_clock := GREATEST(
                pg_catalog.clock_timestamp(),
                deployment_row.updated_at
            );
            IF mutation_kind NOT IN (
                'record_retryable_failure',
                'record_blocked_failure',
                'supersede',
                'cancel'
            ) THEN
                IF deployment_row.controller_id
                        IS DISTINCT FROM expected_controller_id
                    OR deployment_row.controller_fencing_token
                        IS DISTINCT FROM expected_controller_fencing_token
                    OR deployment_row.controller_acquired_at IS NULL
                    OR deployment_row.controller_lease_expires_at IS NULL
                    OR deployment_row.controller_lease_expires_at
                        <= mutation_clock
                    OR deployment_row.snapshot
                        #>> '{controller_lease,controller_id}'
                        IS DISTINCT FROM expected_controller_id
                    OR deployment_row.snapshot
                        #>> '{controller_lease,fencing_token}'
                        IS DISTINCT FROM expected_controller_fencing_token::TEXT
                THEN
                    RAISE EXCEPTION USING
                        ERRCODE = 'RX001',
                        MESSAGE = 'runtime_execution_mutation_replay_ownership_lost';
                END IF;
                IF NOT pg_catalog.pg_input_is_valid(
                        deployment_row.snapshot
                            #>> '{controller_lease,acquired_at}',
                        'timestamp with time zone'
                    )
                    OR NOT pg_catalog.pg_input_is_valid(
                        deployment_row.snapshot
                            #>> '{controller_lease,expires_at}',
                        'timestamp with time zone'
                    )
                THEN
                    RAISE EXCEPTION USING
                        ERRCODE = 'RX004',
                        MESSAGE = 'runtime_execution_mutation_replay_lease_invalid';
                END IF;
                IF (deployment_row.snapshot
                        #>> '{controller_lease,acquired_at}')::TIMESTAMPTZ
                        IS DISTINCT FROM deployment_row.controller_acquired_at
                    OR (deployment_row.snapshot
                        #>> '{controller_lease,expires_at}')::TIMESTAMPTZ
                        IS DISTINCT FROM deployment_row.controller_lease_expires_at
                THEN
                    RAISE EXCEPTION USING
                        ERRCODE = 'RX004',
                        MESSAGE = 'runtime_execution_mutation_replay_lease_invalid';
                END IF;

                authority_outcome := public.starring_runtime_lock_current_authority(
                    deployment_row.activation_request_id,
                    deployment_row.promotion_id,
                    deployment_row.tenant_id,
                    deployment_row.installation_id,
                    deployment_row.installation_authority_revision,
                    deployment_row.guild_id,
                    deployment_row.ruleset_key,
                    deployment_row.target_version,
                    deployment_row.target_content_hash,
                    deployment_row.binding_revision,
                    deployment_row.binding_fingerprint
                );
                IF authority_outcome = 'active_mismatch' THEN
                    RAISE EXCEPTION USING
                        ERRCODE = 'RX006',
                        MESSAGE = 'runtime_execution_mutation_target_superseded';
                ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
                    RAISE EXCEPTION USING
                        ERRCODE = 'RX003',
                        MESSAGE = 'runtime_execution_mutation_authority_changed';
                END IF;
            END IF;

            outcome_name := 'replayed';
            previous_snapshot := deployment_row.snapshot;
            snapshot := deployment_row.snapshot;
            convergence_attempt_no :=
                deployment_row.convergence_attempt_no;
            mutated_at := mutation_clock;
            RETURN NEXT;
            RETURN;
        END IF;

        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_mutation_replay_mismatch';
    END IF;

    IF deployment_row.revision IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.controller_acquired_at IS NULL
        OR deployment_row.controller_lease_expires_at IS NULL
        OR deployment_row.phase IN ('live', 'superseded', 'cancelled')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_mutation_ownership_lost';
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );
    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_mutation_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_mutation_authority_changed';
    END IF;

    IF mutation_kind = 'accept_drain' THEN
        PERFORM pg_catalog.pg_advisory_xact_lock(
            pg_catalog.hashtextextended(
                pg_catalog.concat(
                    'starring-runtime-serving-slot-v1:',
                    deployment_row.guild_id,
                    ':',
                    deployment_row.ruleset_key
                ),
                0
            )
        );
        SELECT lease.*
        INTO serving_row
        FROM public.runtime_serving_leases AS lease
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
        FOR UPDATE;
        serving_found := FOUND;
    END IF;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF deployment_row.controller_lease_expires_at <= mutation_clock THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_mutation_lease_expired';
    END IF;

    previous_snapshot_value := deployment_row.snapshot;
    next_snapshot := deployment_row.snapshot;
    next_revision := expected_deployment_revision + 1;
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );

    IF mutation_kind = 'accept_preflight' THEN
        IF payload_key_count <> 4
            OR NOT mutation_payload ?& ARRAY[
                'target',
                'runtime_generation',
                'observed_runtime',
                'checked_at'
            ]
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'target')
                <> 'object'
            OR pg_catalog.jsonb_typeof(
                mutation_payload -> 'runtime_generation'
            ) <> 'number'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'observed_runtime')
                NOT IN ('object', 'null')
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'checked_at')
                <> 'string'
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload ->> 'checked_at',
                'timestamp with time zone'
            )
            OR mutation_payload ->> 'checked_at'
                !~ '^[0-9]{4}-(0[1-9]|1[0-2])-([0-2][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
            OR mutation_payload ->> 'checked_at'
                ~ '[.][0-9]*000Z$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_preflight_invalid';
        END IF;
        IF mutation_payload -> 'target'
                IS DISTINCT FROM deployment_row.snapshot -> 'target'
            OR (mutation_payload -> 'target')::TEXT
                IS DISTINCT FROM (deployment_row.snapshot -> 'target')::TEXT
            OR mutation_payload ->> 'runtime_generation'
                IS DISTINCT FROM expected_runtime_generation::TEXT
            OR mutation_payload -> 'observed_runtime'
                IS DISTINCT FROM deployment_row.snapshot -> 'previous_runtime'
            OR (mutation_payload -> 'observed_runtime')::TEXT
                IS DISTINCT FROM (
                    deployment_row.snapshot -> 'previous_runtime'
                )::TEXT
            OR (mutation_payload ->> 'checked_at')::TIMESTAMPTZ
                < deployment_row.requested_at
            OR (mutation_payload ->> 'checked_at')::TIMESTAMPTZ
                > mutation_clock + INTERVAL '30 seconds'
            OR deployment_row.phase <> 'requested'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_preflight_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{preflight}',
            mutation_payload,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"preflight_ready"}'::JSONB,
            FALSE
        );
        next_phase := 'preflight_ready';
    ELSIF mutation_kind = 'request_drain' THEN
        IF mutation_payload <> '{}'::JSONB
            OR deployment_row.phase <> 'preflight_ready'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_request_drain_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"drain_requested"}'::JSONB,
            FALSE
        );
        next_phase := 'drain_requested';
    ELSIF mutation_kind = 'accept_drain' THEN
        IF payload_key_count <> 3
            OR NOT mutation_payload ?& ARRAY[
                'previous_runtime',
                'target_runtime_generation',
                'drained_at'
            ]
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'previous_runtime')
                NOT IN ('object', 'null')
            OR pg_catalog.jsonb_typeof(
                mutation_payload -> 'target_runtime_generation'
            ) <> 'number'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'drained_at')
                <> 'string'
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload ->> 'drained_at',
                'timestamp with time zone'
            )
            OR mutation_payload ->> 'drained_at'
                !~ '^[0-9]{4}-(0[1-9]|1[0-2])-([0-2][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
            OR mutation_payload ->> 'drained_at'
                ~ '[.][0-9]*000Z$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_drain_invalid';
        END IF;
        IF mutation_payload -> 'previous_runtime'
                IS DISTINCT FROM deployment_row.snapshot -> 'previous_runtime'
            OR (mutation_payload -> 'previous_runtime')::TEXT
                IS DISTINCT FROM (
                    deployment_row.snapshot -> 'previous_runtime'
                )::TEXT
            OR mutation_payload ->> 'target_runtime_generation'
                IS DISTINCT FROM expected_runtime_generation::TEXT
            OR deployment_row.phase <> 'drain_requested'
            OR (mutation_payload ->> 'drained_at')::TIMESTAMPTZ
                < (deployment_row.snapshot
                    #>> '{preflight,checked_at}')::TIMESTAMPTZ
            OR (mutation_payload ->> 'drained_at')::TIMESTAMPTZ
                > mutation_clock + INTERVAL '30 seconds'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_drain_invalid';
        END IF;

        IF serving_found THEN
            IF serving_row.acquired_at > serving_row.last_heartbeat_at
                OR serving_row.last_heartbeat_at > serving_row.expires_at
                OR serving_row.acquired_at > mutation_clock
                OR serving_row.serving IS DISTINCT FROM serving_row.connected
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX004',
                    MESSAGE = 'runtime_execution_drain_serving_state_invalid';
            END IF;
            closure_boundary := CASE
                WHEN NOT serving_row.connected
                    AND serving_row.last_heartbeat_at = serving_row.expires_at
                    AND serving_row.expires_at <= mutation_clock
                    THEN serving_row.last_heartbeat_at
                WHEN serving_row.connected
                    AND serving_row.serving
                    AND serving_row.expires_at <= mutation_clock
                    THEN serving_row.expires_at
            END;
            IF closure_boundary IS NULL THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX001',
                    MESSAGE = 'runtime_execution_drain_serving_active';
            END IF;
        END IF;

        IF deployment_row.previous_runtime IS NULL THEN
            IF serving_found
                AND closure_boundary > (mutation_payload ->> 'drained_at')::TIMESTAMPTZ
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX002',
                    MESSAGE = 'runtime_execution_drain_time_regression';
            END IF;
        ELSIF NOT serving_found
            OR serving_row.tenant_id
                IS DISTINCT FROM expected_tenant_id
            OR serving_row.installation_id
                IS DISTINCT FROM expected_installation_id
            OR serving_row.deployment_id
                IS NOT DISTINCT FROM expected_deployment_id
            OR serving_row.guild_id
                IS DISTINCT FROM deployment_row.previous_runtime
                    #>> '{target,guild_id}'
            OR serving_row.ruleset_key
                IS DISTINCT FROM deployment_row.previous_runtime
                    #>> '{target,ruleset_key}'
            OR serving_row.target_version
                IS DISTINCT FROM (
                    deployment_row.previous_runtime
                        #>> '{target,version}'
                )::BIGINT
            OR serving_row.target_content_hash
                IS DISTINCT FROM deployment_row.previous_runtime
                    #>> '{target,content_hash}'
            OR serving_row.binding_revision
                IS DISTINCT FROM (
                    deployment_row.previous_runtime
                        #>> '{target,binding_revision}'
                )::BIGINT
            OR serving_row.binding_fingerprint
                IS DISTINCT FROM deployment_row.previous_runtime
                    #>> '{target,binding_fingerprint}'
            OR serving_row.runtime_generation
                IS DISTINCT FROM (
                    deployment_row.previous_runtime
                        ->> 'runtime_generation'
                )::BIGINT
            OR serving_row.process_instance_id
                IS DISTINCT FROM deployment_row.previous_runtime
                    ->> 'process_instance_id'
            OR serving_row.acquired_at > deployment_row.requested_at
            OR closure_boundary IS NULL
            OR closure_boundary < deployment_row.requested_at
            OR closure_boundary
                > (mutation_payload ->> 'drained_at')::TIMESTAMPTZ
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX001',
                MESSAGE = 'runtime_execution_drain_previous_runtime_mismatch';
        END IF;

        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{drain}',
            mutation_payload,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"drained"}'::JSONB,
            FALSE
        );
        next_phase := 'drained';
    ELSIF mutation_kind = 'begin_activation' THEN
        IF mutation_payload <> '{}'::JSONB
            OR deployment_row.phase <> 'drained'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_begin_activation_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"activation_applying"}'::JSONB,
            FALSE
        );
        next_phase := 'activation_applying';
    ELSIF mutation_kind = 'accept_activation' THEN
        IF payload_key_count <> 5
            OR NOT mutation_payload ?& ARRAY[
                'activation_request_id',
                'target',
                'runtime_generation',
                'kind',
                'activated_at'
            ]
            OR pg_catalog.jsonb_typeof(
                mutation_payload -> 'activation_request_id'
            ) <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'target')
                <> 'object'
            OR pg_catalog.jsonb_typeof(
                mutation_payload -> 'runtime_generation'
            ) <> 'number'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'kind')
                <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'activated_at')
                <> 'string'
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload ->> 'activated_at',
                'timestamp with time zone'
            )
            OR mutation_payload ->> 'activated_at'
                !~ '^[0-9]{4}-(0[1-9]|1[0-2])-([0-2][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
            OR mutation_payload ->> 'activated_at'
                ~ '[.][0-9]*000Z$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_activation_invalid';
        END IF;
        IF mutation_payload ->> 'activation_request_id'
                IS DISTINCT FROM deployment_row.activation_request_id
            OR mutation_payload -> 'target'
                IS DISTINCT FROM deployment_row.snapshot -> 'target'
            OR (mutation_payload -> 'target')::TEXT
                IS DISTINCT FROM (deployment_row.snapshot -> 'target')::TEXT
            OR mutation_payload ->> 'runtime_generation'
                IS DISTINCT FROM expected_runtime_generation::TEXT
            OR mutation_payload ->> 'kind'
                NOT IN ('activated', 'already_active', 'crash_recovered')
            OR (mutation_payload ->> 'activated_at')::TIMESTAMPTZ
                < (deployment_row.snapshot
                    #>> '{drain,drained_at}')::TIMESTAMPTZ
            OR (mutation_payload ->> 'activated_at')::TIMESTAMPTZ
                > mutation_clock + INTERVAL '30 seconds'
            OR deployment_row.phase <> 'activation_applying'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_activation_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{activation}',
            mutation_payload,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"runtime_pending","condition":{"condition":"ready"}}'::JSONB,
            FALSE
        );
        next_phase := 'runtime_pending';
    ELSIF mutation_kind IN (
        'record_retryable_failure',
        'record_blocked_failure'
    ) THEN
        IF deployment_row.phase NOT IN (
                'runtime_pending',
                'reconciling_panels',
                'awaiting_gateway_ready'
            )
            OR (
                deployment_row.phase = 'runtime_pending'
                AND deployment_row.snapshot
                    #>> '{phase,condition,condition}' IS DISTINCT FROM 'ready'
            )
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'failure_id')
                <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'kind')
                <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'code')
                <> 'string'
            OR mutation_payload ->> 'failure_id'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload ->> 'kind' NOT IN (
                'environment_unavailable',
                'activation_not_observable',
                'panel_reconciliation',
                'gateway_start',
                'gateway_ready_timeout',
                'invariant_violation'
            )
            OR mutation_payload ->> 'code' !~ '^[a-z0-9_]{1,64}$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_failure_invalid';
        END IF;

        IF pg_catalog.jsonb_typeof(
                deployment_row.snapshot -> 'activation'
            ) <> 'object'
            OR NOT pg_catalog.pg_input_is_valid(
                deployment_row.snapshot #>> '{activation,activated_at}',
                'timestamp with time zone'
            )
            OR (
                pg_catalog.jsonb_typeof(
                    deployment_row.snapshot -> 'last_live_recovery'
                ) IS DISTINCT FROM 'null'
                AND (
                    pg_catalog.jsonb_typeof(
                        deployment_row.snapshot -> 'last_live_recovery'
                    ) <> 'object'
                    OR NOT pg_catalog.pg_input_is_valid(
                        deployment_row.snapshot
                            #>> '{last_live_recovery,recovered_at}',
                        'timestamp with time zone'
                    )
                )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_failure_evidence_invalid';
        END IF;

        IF mutation_clock < (
                deployment_row.snapshot #>> '{activation,activated_at}'
            )::TIMESTAMPTZ
            OR (
                pg_catalog.jsonb_typeof(
                    deployment_row.snapshot -> 'last_live_recovery'
                ) = 'object'
                AND mutation_clock < (
                    deployment_row.snapshot
                        #>> '{last_live_recovery,recovered_at}'
                )::TIMESTAMPTZ
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE = 'runtime_execution_failure_evidence_not_ready';
        END IF;

        stable_message := CASE mutation_payload ->> 'kind'
            WHEN 'environment_unavailable'
                THEN 'runtime environment unavailable'
            WHEN 'activation_not_observable'
                THEN 'activation not observable'
            WHEN 'panel_reconciliation'
                THEN 'panel reconciliation failed'
            WHEN 'gateway_start'
                THEN 'gateway start failed'
            WHEN 'gateway_ready_timeout'
                THEN 'gateway Ready timed out'
            WHEN 'invariant_violation'
                THEN 'runtime invariant rejected'
        END;
        failure_value := pg_catalog.jsonb_build_object(
            'failure_id', mutation_payload ->> 'failure_id',
            'kind', mutation_payload ->> 'kind',
            'code', mutation_payload ->> 'code',
            'message', stable_message,
            'recorded_at', mutation_clock
        );

        IF mutation_kind = 'record_retryable_failure' THEN
            IF payload_key_count <> 5
                OR NOT mutation_payload ?& ARRAY[
                    'failure_id',
                    'kind',
                    'code',
                    'attempt',
                    'retry_after_milliseconds'
                ]
                OR pg_catalog.jsonb_typeof(mutation_payload -> 'attempt')
                    <> 'number'
                OR pg_catalog.jsonb_typeof(
                    mutation_payload -> 'retry_after_milliseconds'
                ) <> 'number'
                OR NOT pg_catalog.pg_input_is_valid(
                    mutation_payload ->> 'attempt',
                    'bigint'
                )
                OR NOT pg_catalog.pg_input_is_valid(
                    mutation_payload ->> 'retry_after_milliseconds',
                    'bigint'
                )
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX002',
                    MESSAGE = 'runtime_execution_retry_failure_invalid';
            END IF;
            IF (mutation_payload ->> 'attempt')::BIGINT
                    IS DISTINCT FROM expected_convergence_attempt_no
                OR (mutation_payload ->> 'retry_after_milliseconds')::BIGINT
                    NOT BETWEEN 1 AND 86400000
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX002',
                    MESSAGE = 'runtime_execution_retry_failure_invalid';
            END IF;
            retry_duration :=
                (mutation_payload ->> 'retry_after_milliseconds')::BIGINT
                * INTERVAL '1 millisecond';
            retry_not_before := mutation_clock + retry_duration;
            disposition_value := pg_catalog.jsonb_build_object(
                'disposition', 'retryable',
                'failure', failure_value,
                'attempt', expected_convergence_attempt_no,
                'retry_not_before', retry_not_before
            );
            next_snapshot := pg_catalog.jsonb_set(
                next_snapshot,
                '{phase}',
                pg_catalog.jsonb_build_object(
                    'phase', 'runtime_pending',
                    'condition', pg_catalog.jsonb_build_object(
                        'condition', 'retryable',
                        'failure', failure_value,
                        'attempt', expected_convergence_attempt_no,
                        'retry_not_before', retry_not_before
                    )
                ),
                FALSE
            );
        ELSE
            IF payload_key_count <> 3
                OR NOT mutation_payload ?& ARRAY[
                    'failure_id',
                    'kind',
                    'code'
                ]
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX002',
                    MESSAGE = 'runtime_execution_blocked_failure_invalid';
            END IF;
            disposition_value := pg_catalog.jsonb_build_object(
                'disposition', 'blocked',
                'failure', failure_value
            );
            next_snapshot := pg_catalog.jsonb_set(
                next_snapshot,
                '{phase}',
                pg_catalog.jsonb_build_object(
                    'phase', 'runtime_pending',
                    'condition', pg_catalog.jsonb_build_object(
                        'condition', 'blocked',
                        'failure', failure_value
                    )
                ),
                FALSE
            );
        END IF;

        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{last_runtime_failure}',
            disposition_value,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{panel_certificate}',
            'null'::JSONB,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{gateway_ready}',
            'null'::JSONB,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{live}',
            'null'::JSONB,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{controller_lease}',
            'null'::JSONB,
            FALSE
        );
        next_phase := 'runtime_pending';
        release_controller := TRUE;
        records_failure := TRUE;
    ELSIF mutation_kind = 'resume_runtime_pending' THEN
        IF mutation_payload <> '{}'::JSONB
            OR deployment_row.phase <> 'runtime_pending'
            OR deployment_row.snapshot
                #>> '{phase,condition,condition}' <> 'retryable'
            OR deployment_row.last_failure_attempt_no IS NULL
            OR deployment_row.last_failure_attempt_no
                >= expected_convergence_attempt_no
            OR deployment_row.next_retry_at IS NULL
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_resume_invalid';
        END IF;
        IF deployment_row.next_retry_at > mutation_clock THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE = 'runtime_execution_resume_retry_not_ready';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"runtime_pending","condition":{"condition":"ready"}}'::JSONB,
            FALSE
        );
        next_phase := 'runtime_pending';
    ELSIF mutation_kind = 'begin_panel_reconciliation' THEN
        IF mutation_payload <> '{}'::JSONB
            OR deployment_row.phase <> 'runtime_pending'
            OR deployment_row.snapshot
                #>> '{phase,condition,condition}' <> 'ready'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_begin_panel_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"reconciling_panels"}'::JSONB,
            FALSE
        );
        next_phase := 'reconciling_panels';
    ELSIF mutation_kind = 'accept_panel_certificate' THEN
        IF payload_key_count <> 16
            OR NOT mutation_payload ?& ARRAY[
                'certificate_id',
                'report_digest',
                'target',
                'runtime_generation',
                'process_instance_id',
                'declared_count',
                'installed_count',
                'unchanged_count',
                'skipped_transient_count',
                'skipped_unresolved_channel_count',
                'failed_count',
                'ambiguous_outcome_count',
                'stale_message_cleanup_pending_count',
                'orphan_message_cleanup_pending_count',
                'reposted_old_message_cleanup_pending_count',
                'reconciled_at'
            ]
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'certificate_id')
                <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'report_digest')
                <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'target')
                <> 'object'
            OR pg_catalog.jsonb_typeof(
                mutation_payload -> 'runtime_generation'
            ) <> 'number'
            OR pg_catalog.jsonb_typeof(
                mutation_payload -> 'process_instance_id'
            ) <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'reconciled_at')
                <> 'string'
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_each(mutation_payload) AS item
                WHERE item.key IN (
                    'declared_count',
                    'installed_count',
                    'unchanged_count',
                    'skipped_transient_count',
                    'skipped_unresolved_channel_count',
                    'failed_count',
                    'ambiguous_outcome_count',
                    'stale_message_cleanup_pending_count',
                    'orphan_message_cleanup_pending_count',
                    'reposted_old_message_cleanup_pending_count'
                )
                AND (
                    pg_catalog.jsonb_typeof(item.value) <> 'number'
                    OR item.value::TEXT !~ '^[0-9]{1,10}$'
                )
            )
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload ->> 'reconciled_at',
                'timestamp with time zone'
            )
            OR mutation_payload ->> 'reconciled_at'
                !~ '^[0-9]{4}-(0[1-9]|1[0-2])-([0-2][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
            OR mutation_payload ->> 'reconciled_at'
                ~ '[.][0-9]*000Z$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_panel_certificate_invalid';
        END IF;
        IF payload_key_count <> 16
            OR NOT mutation_payload ?& ARRAY[
                'certificate_id',
                'report_digest',
                'target',
                'runtime_generation',
                'process_instance_id',
                'declared_count',
                'installed_count',
                'unchanged_count',
                'skipped_transient_count',
                'skipped_unresolved_channel_count',
                'failed_count',
                'ambiguous_outcome_count',
                'stale_message_cleanup_pending_count',
                'orphan_message_cleanup_pending_count',
                'reposted_old_message_cleanup_pending_count',
                'reconciled_at'
            ]
            OR mutation_payload ->> 'certificate_id'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload ->> 'report_digest' !~ '^[0-9a-f]{64}$'
            OR mutation_payload -> 'target'
                IS DISTINCT FROM deployment_row.snapshot -> 'target'
            OR (mutation_payload -> 'target')::TEXT
                IS DISTINCT FROM (deployment_row.snapshot -> 'target')::TEXT
            OR mutation_payload ->> 'runtime_generation'
                IS DISTINCT FROM expected_runtime_generation::TEXT
            OR mutation_payload ->> 'process_instance_id'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload ->> 'reconciled_at',
                'timestamp with time zone'
            )
            OR (mutation_payload ->> 'reconciled_at')::TIMESTAMPTZ
                < (deployment_row.snapshot
                    #>> '{activation,activated_at}')::TIMESTAMPTZ
            OR (mutation_payload ->> 'reconciled_at')::TIMESTAMPTZ
                > mutation_clock + INTERVAL '30 seconds'
            OR deployment_row.phase <> 'reconciling_panels'
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_each_text(mutation_payload) AS item
                WHERE item.key IN (
                    'declared_count',
                    'installed_count',
                    'unchanged_count',
                    'skipped_transient_count',
                    'skipped_unresolved_channel_count',
                    'failed_count',
                    'ambiguous_outcome_count',
                    'stale_message_cleanup_pending_count',
                    'orphan_message_cleanup_pending_count',
                    'reposted_old_message_cleanup_pending_count'
                )
                AND (
                    item.value !~ '^[0-9]{1,10}$'
                    OR item.value::NUMERIC > 4294967295
                )
            )
            OR mutation_payload ->> 'skipped_transient_count' <> '0'
            OR mutation_payload ->> 'skipped_unresolved_channel_count' <> '0'
            OR mutation_payload ->> 'failed_count' <> '0'
            OR mutation_payload ->> 'ambiguous_outcome_count' <> '0'
            OR mutation_payload ->> 'stale_message_cleanup_pending_count' <> '0'
            OR mutation_payload ->> 'orphan_message_cleanup_pending_count' <> '0'
            OR mutation_payload ->> 'reposted_old_message_cleanup_pending_count'
                <> '0'
            OR (mutation_payload ->> 'installed_count')::NUMERIC
                + (mutation_payload ->> 'unchanged_count')::NUMERIC
                <> (mutation_payload ->> 'declared_count')::NUMERIC
            OR (
                deployment_row.snapshot -> 'last_live_recovery' <> 'null'::JSONB
                AND (
                    (mutation_payload ->> 'reconciled_at')::TIMESTAMPTZ
                        < (deployment_row.snapshot
                            #>> '{last_live_recovery,recovered_at}')::TIMESTAMPTZ
                    OR mutation_payload ->> 'process_instance_id'
                        IS NOT DISTINCT FROM deployment_row.snapshot
                            #>> '{last_live_recovery,prior_live,process_instance_id}'
                )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_panel_certificate_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{panel_certificate}',
            mutation_payload,
            FALSE
        );
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            '{"phase":"awaiting_gateway_ready"}'::JSONB,
            FALSE
        );
        next_phase := 'awaiting_gateway_ready';
    ELSIF mutation_kind = 'supersede' THEN
        IF payload_key_count <> 2
            OR NOT mutation_payload ?& ARRAY['by', 'reason']
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'by') <> 'object'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'reason')
                <> 'string'
            OR pg_catalog.jsonb_typeof(mutation_payload #> '{by,identity}')
                <> 'object'
            OR pg_catalog.jsonb_typeof(mutation_payload #> '{by,target}')
                <> 'object'
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,runtime_generation}'
            ) <> 'number'
            OR pg_catalog.octet_length(mutation_payload ->> 'reason') > 1024
            OR pg_catalog.translate(
                mutation_payload ->> 'reason',
                reason_trim_characters,
                ''
            ) = ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_supersede_invalid';
        END IF;

        SELECT pg_catalog.count(*)
        INTO nested_key_count
        FROM pg_catalog.jsonb_object_keys(mutation_payload -> 'by');
        SELECT pg_catalog.count(*)
        INTO identity_key_count
        FROM pg_catalog.jsonb_object_keys(
            mutation_payload #> '{by,identity}'
        );
        SELECT pg_catalog.count(*)
        INTO target_key_count
        FROM pg_catalog.jsonb_object_keys(
            mutation_payload #> '{by,target}'
        );

        IF nested_key_count <> 3
            OR identity_key_count <> 5
            OR target_key_count <> 6
            OR NOT ((mutation_payload -> 'by')
                ?& ARRAY['identity', 'target', 'runtime_generation'])
            OR NOT ((mutation_payload #> '{by,identity}') ?& ARRAY[
                'deployment_id',
                'tenant_id',
                'installation_id',
                'promotion_id',
                'activation_request_id'
            ])
            OR NOT ((mutation_payload #> '{by,target}') ?& ARRAY[
                'guild_id',
                'ruleset_key',
                'version',
                'content_hash',
                'binding_revision',
                'binding_fingerprint'
            ])
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_each(
                    mutation_payload #> '{by,identity}'
                ) AS item
                WHERE pg_catalog.jsonb_typeof(item.value) <> 'string'
            )
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,target,guild_id}'
            ) <> 'string'
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,target,ruleset_key}'
            ) <> 'string'
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,target,version}'
            ) <> 'number'
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,target,content_hash}'
            ) <> 'string'
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,target,binding_revision}'
            ) <> 'number'
            OR pg_catalog.jsonb_typeof(
                mutation_payload #> '{by,target,binding_fingerprint}'
            ) <> 'string'
            OR mutation_payload #>> '{by,identity,deployment_id}'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload #>> '{by,identity,tenant_id}'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload #>> '{by,identity,installation_id}'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload #>> '{by,identity,promotion_id}'
                !~ '^[0-9a-f]{64}$'
            OR mutation_payload #>> '{by,identity,activation_request_id}'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload #>> '{by,target,guild_id}'
                !~ '^[1-9][0-9]{0,19}$'
            OR mutation_payload #>> '{by,target,ruleset_key}'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR mutation_payload #>> '{by,target,content_hash}'
                !~ '^[0-9a-f]{64}$'
            OR mutation_payload #>> '{by,target,binding_fingerprint}'
                !~ '^[0-9a-f]{64}$'
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload #>> '{by,runtime_generation}',
                'bigint'
            )
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload #>> '{by,target,version}',
                'bigint'
            )
            OR NOT pg_catalog.pg_input_is_valid(
                mutation_payload #>> '{by,target,binding_revision}',
                'bigint'
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_supersede_invalid';
        END IF;
        IF mutation_payload #>> '{by,identity,deployment_id}'
                = expected_deployment_id
            OR mutation_payload #>> '{by,identity,tenant_id}'
                IS DISTINCT FROM expected_tenant_id
            OR mutation_payload #>> '{by,identity,installation_id}'
                IS DISTINCT FROM expected_installation_id
            OR mutation_payload #>> '{by,target,guild_id}'
                IS DISTINCT FROM deployment_row.guild_id
            OR mutation_payload #>> '{by,target,ruleset_key}'
                IS DISTINCT FROM deployment_row.ruleset_key
            OR (mutation_payload #>> '{by,target,version}')::BIGINT
                NOT BETWEEN 1 AND 4294967295
            OR (mutation_payload #>> '{by,target,binding_revision}')::BIGINT
                NOT BETWEEN 1 AND 9223372036854775807
            OR (mutation_payload #>> '{by,runtime_generation}')::BIGINT
                <= expected_runtime_generation
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_supersede_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            pg_catalog.jsonb_build_object(
                'phase', 'superseded',
                'by', mutation_payload -> 'by',
                'reason', mutation_payload ->> 'reason',
                'superseded_at', mutation_clock
            ),
            FALSE
        );
        next_phase := 'superseded';
        release_controller := TRUE;
    ELSIF mutation_kind = 'cancel' THEN
        IF payload_key_count <> 1
            OR NOT mutation_payload ? 'reason'
            OR pg_catalog.jsonb_typeof(mutation_payload -> 'reason')
                <> 'string'
            OR pg_catalog.octet_length(mutation_payload ->> 'reason') > 1024
            OR pg_catalog.translate(
                mutation_payload ->> 'reason',
                reason_trim_characters,
                ''
            ) = ''
            OR deployment_row.phase NOT IN (
                'requested',
                'preflight_ready',
                'drain_requested'
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_execution_cancel_invalid';
        END IF;
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{phase}',
            pg_catalog.jsonb_build_object(
                'phase', 'cancelled',
                'reason', mutation_payload ->> 'reason',
                'cancelled_at', mutation_clock
            ),
            FALSE
        );
        next_phase := 'cancelled';
        release_controller := TRUE;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_mutation_input_invalid';
    END IF;

    IF release_controller THEN
        next_snapshot := pg_catalog.jsonb_set(
            next_snapshot,
            '{controller_lease}',
            'null'::JSONB,
            FALSE
        );
    END IF;

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        phase = next_phase,
        controller_id = CASE
            WHEN release_controller THEN NULL
            ELSE deployment.controller_id
        END,
        controller_fencing_token = CASE
            WHEN release_controller THEN NULL
            ELSE deployment.controller_fencing_token
        END,
        controller_acquired_at = CASE
            WHEN release_controller THEN NULL
            ELSE deployment.controller_acquired_at
        END,
        controller_lease_expires_at = CASE
            WHEN release_controller THEN NULL
            ELSE deployment.controller_lease_expires_at
        END,
        next_retry_at = CASE
            WHEN mutation_kind = 'record_retryable_failure'
                THEN retry_not_before
            ELSE NULL
        END,
        last_stable_error_code = CASE
            WHEN records_failure THEN mutation_payload ->> 'code'
            ELSE next_snapshot #>> '{last_runtime_failure,failure,code}'
        END,
        blocked_at = CASE
            WHEN mutation_kind = 'record_blocked_failure'
                THEN mutation_clock
            ELSE NULL
        END,
        superseded_at = CASE
            WHEN mutation_kind = 'supersede' THEN mutation_clock
            ELSE NULL
        END,
        cancelled_at = CASE
            WHEN mutation_kind = 'cancel' THEN mutation_clock
            ELSE NULL
        END,
        last_failure_attempt_no = CASE
            WHEN records_failure THEN expected_convergence_attempt_no
            ELSE deployment.last_failure_attempt_no
        END,
        last_controller_id = expected_controller_id,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
        AND deployment.revision = expected_deployment_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_mutation_ownership_lost';
    END IF;

    INSERT INTO public.runtime_execution_mutation_markers AS marker (
        deployment_id,
        mutation_revision,
        mutation_kind,
        mutation_payload
    ) VALUES (
        expected_deployment_id,
        next_revision,
        mutation_kind,
        mutation_payload
    )
    ON CONFLICT (deployment_id) DO UPDATE
    SET mutation_revision = EXCLUDED.mutation_revision,
        mutation_kind = EXCLUDED.mutation_kind,
        mutation_payload = EXCLUDED.mutation_payload
    WHERE marker.mutation_revision < EXCLUDED.mutation_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_mutation_marker_mismatch';
    END IF;

    outcome_name := 'applied';
    previous_snapshot := previous_snapshot_value;
    snapshot := next_snapshot;
    convergence_attempt_no := expected_convergence_attempt_no;
    mutated_at := mutation_clock;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_execution_certify_prepare_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_gateway_ready JSONB,
    expected_runtime_build_revision TEXT,
    expected_panel_report_digest TEXT,
    expected_gateway_shard_id TEXT,
    requested_serving_lease_milliseconds BIGINT
)
RETURNS TABLE(
    preparation_name TEXT,
    observed_snapshot JSONB,
    convergence_attempt_no BIGINT,
    mutation_clock TIMESTAMPTZ,
    certified_at TIMESTAMPTZ
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
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    attestation_row public.runtime_attestations%ROWTYPE;
    authority_outcome TEXT;
    canonical_artifact BOOLEAN;
    prepared_clock TIMESTAMPTZ;
    key_count BIGINT;
    serving_found BOOLEAN;
    attestation_found BOOLEAN;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.jsonb_typeof(expected_gateway_ready) <> 'object'
        OR expected_runtime_build_revision
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_panel_report_digest !~ '^[0-9a-f]{64}$'
        OR expected_gateway_shard_id
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR requested_serving_lease_milliseconds
            NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_prepare_input_invalid';
    END IF;

    SELECT pg_catalog.count(*)
    INTO key_count
    FROM pg_catalog.jsonb_object_keys(expected_gateway_ready);

    IF key_count <> 5
        OR NOT expected_gateway_ready ?& ARRAY[
            'target',
            'runtime_generation',
            'process_instance_id',
            'kind',
            'ready_at'
        ]
        OR pg_catalog.jsonb_typeof(expected_gateway_ready -> 'target')
            <> 'object'
        OR pg_catalog.jsonb_typeof(
            expected_gateway_ready -> 'runtime_generation'
        ) <> 'number'
        OR pg_catalog.jsonb_typeof(
            expected_gateway_ready -> 'process_instance_id'
        ) <> 'string'
        OR pg_catalog.jsonb_typeof(expected_gateway_ready -> 'kind')
            <> 'string'
        OR pg_catalog.jsonb_typeof(expected_gateway_ready -> 'ready_at')
            <> 'string'
        OR expected_gateway_ready ->> 'process_instance_id'
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_ready ->> 'kind'
            NOT IN ('discord_ready', 'discord_resumed')
        OR NOT pg_catalog.pg_input_is_valid(
            expected_gateway_ready ->> 'ready_at',
            'timestamp with time zone'
        )
        OR expected_gateway_ready ->> 'ready_at'
            !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_prepare_input_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND
        OR deployment_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR expected_gateway_ready -> 'target'
            IS DISTINCT FROM deployment_row.snapshot -> 'target'
        OR expected_gateway_ready ->> 'runtime_generation'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR expected_gateway_ready ->> 'process_instance_id'
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,process_instance_id}'
        OR expected_panel_report_digest
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,report_digest}'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_prepare_ownership_lost';
    END IF;

    IF deployment_row.revision = expected_deployment_revision + 1
        AND deployment_row.phase = 'live'
        AND deployment_row.last_controller_id
            IS NOT DISTINCT FROM expected_controller_id
        AND deployment_row.last_fencing_token
            IS NOT DISTINCT FROM expected_controller_fencing_token
        AND deployment_row.snapshot -> 'gateway_ready'
            IS NOT DISTINCT FROM expected_gateway_ready
    THEN
        PERFORM pg_catalog.pg_advisory_xact_lock(
            pg_catalog.hashtextextended(
                pg_catalog.concat(
                    'starring-runtime-serving-slot-v1:',
                    deployment_row.guild_id,
                    ':',
                    deployment_row.ruleset_key
                ),
                0
            )
        );
        SELECT lease.*
        INTO serving_row
        FROM public.runtime_serving_leases AS lease
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
        FOR UPDATE;
        serving_found := FOUND;

        SELECT attestation.*
        INTO attestation_row
        FROM public.runtime_attestations AS attestation
        WHERE attestation.tenant_id = expected_tenant_id
            AND attestation.installation_id = expected_installation_id
            AND attestation.deployment_id = expected_deployment_id
            AND attestation.attestation_id
                = deployment_row.live_attestation_id
        FOR KEY SHARE;
        attestation_found := FOUND;

        prepared_clock := public.starring_runtime_mutation_clock();
        IF NOT serving_found
            OR NOT attestation_found
            OR attestation_row.runtime_build_revision
                IS DISTINCT FROM expected_runtime_build_revision
            OR attestation_row.panel_report_digest
                IS DISTINCT FROM expected_panel_report_digest
            OR attestation_row.gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR attestation_row.convergence_attempt_no
                IS DISTINCT FROM expected_convergence_attempt_no
            OR attestation_row.serving_lease_duration_nanos
                IS DISTINCT FROM
                    requested_serving_lease_milliseconds * 1000000
            OR serving_row.attestation_id
                IS DISTINCT FROM attestation_row.attestation_id
            OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR serving_row.installation_id
                IS DISTINCT FROM expected_installation_id
            OR serving_row.deployment_id
                IS DISTINCT FROM expected_deployment_id
            OR serving_row.process_instance_id
                IS DISTINCT FROM expected_gateway_ready
                    ->> 'process_instance_id'
            OR serving_row.runtime_generation
                IS DISTINCT FROM expected_runtime_generation
            OR NOT serving_row.connected
            OR NOT serving_row.serving
            OR serving_row.expires_at <= prepared_clock
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_certify_prepare_replay_mismatch';
        END IF;

        preparation_name := 'replayed';
        observed_snapshot := deployment_row.snapshot;
        convergence_attempt_no := expected_convergence_attempt_no;
        mutation_clock := prepared_clock;
        certified_at := (
            deployment_row.snapshot #>> '{live,certified_at}'
        )::TIMESTAMPTZ;
        RETURN NEXT;
        RETURN;
    END IF;

    IF deployment_row.revision IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR deployment_row.controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_lease_expires_at IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_prepare_ownership_lost';
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );
    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_certify_prepare_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_certify_prepare_authority_changed';
    END IF;

    SELECT version.content_hash = deployment_row.target_content_hash
        AND version.canonical_content_hash
            = deployment_row.target_content_hash
        AND version.schema_version = 1
    INTO canonical_artifact
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = deployment_row.guild_id
        AND version.ruleset_key = deployment_row.ruleset_key
        AND version.version = deployment_row.target_version
    FOR SHARE;

    IF canonical_artifact IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_prepare_artifact_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                deployment_row.guild_id,
                ':',
                deployment_row.ruleset_key
            ),
            0
        )
    );
    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    serving_found := FOUND;

    prepared_clock := public.starring_runtime_mutation_clock();
    IF deployment_row.controller_lease_expires_at <= prepared_clock THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_prepare_lease_expired';
    END IF;
    IF (expected_gateway_ready ->> 'ready_at')::TIMESTAMPTZ
            < (deployment_row.snapshot
                #>> '{panel_certificate,reconciled_at}')::TIMESTAMPTZ
        OR (expected_gateway_ready ->> 'ready_at')::TIMESTAMPTZ
            < prepared_clock - INTERVAL '90 seconds'
        OR (expected_gateway_ready ->> 'ready_at')::TIMESTAMPTZ
            > prepared_clock + INTERVAL '30 seconds'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_prepare_gateway_invalid';
    END IF;

    IF serving_found
        AND serving_row.expires_at > prepared_clock
        AND (serving_row.connected OR serving_row.serving)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_prepare_serving_conflict';
    END IF;

    preparation_name := 'apply';
    observed_snapshot := deployment_row.snapshot;
    convergence_attempt_no := expected_convergence_attempt_no;
    mutation_clock := prepared_clock;
    certified_at := prepared_clock;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_execution_certify_commit_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_gateway_ready JSONB,
    expected_runtime_build_revision TEXT,
    expected_panel_report_digest TEXT,
    expected_gateway_shard_id TEXT,
    requested_serving_lease_milliseconds BIGINT,
    expected_mutation_clock TIMESTAMPTZ,
    expected_observed_snapshot JSONB,
    proposed_attestation_id TEXT,
    proposed_attestation_record JSONB,
    proposed_attestation_record_bytes TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    previous_snapshot JSONB,
    snapshot JSONB,
    convergence_attempt_no BIGINT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    attestation_id TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    serving_revision BIGINT,
    acquired_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    connected BOOLEAN,
    serving BOOLEAN
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
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    existing_attestation public.runtime_attestations%ROWTYPE;
    next_snapshot JSONB;
    live_value JSONB;
    expected_record JSONB;
    authority_outcome TEXT;
    canonical_artifact BOOLEAN;
    current_clock TIMESTAMPTZ;
    requested_duration INTERVAL;
    next_expiry TIMESTAMPTZ;
    next_epoch BIGINT;
    next_serving_revision BIGINT;
    computed_attestation_id TEXT;
    record_bytes BYTEA;
    domain_bytes BYTEA;
    record_key_count BIGINT;
    gateway_key_count BIGINT;
    serving_found BOOLEAN;
    attestation_found BOOLEAN;
    canonical_target_bytes TEXT;
    canonical_activation_bytes TEXT;
    canonical_panel_bytes TEXT;
    canonical_gateway_bytes TEXT;
    canonical_live_bytes TEXT;
    canonical_record_bytes TEXT;
    canonical_activation_time TEXT;
    canonical_panel_time TEXT;
    canonical_gateway_time TEXT;
    canonical_certified_time TEXT;
    activation_time TIMESTAMPTZ;
    panel_time TIMESTAMPTZ;
    gateway_time TIMESTAMPTZ;
    certified_time TIMESTAMPTZ;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.jsonb_typeof(expected_gateway_ready) <> 'object'
        OR pg_catalog.jsonb_typeof(expected_observed_snapshot) <> 'object'
        OR expected_runtime_build_revision
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_panel_report_digest !~ '^[0-9a-f]{64}$'
        OR expected_gateway_shard_id
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR proposed_attestation_id !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(proposed_attestation_record) <> 'object'
        OR pg_catalog.octet_length(proposed_attestation_record::TEXT) > 262144
        OR pg_catalog.octet_length(proposed_attestation_record_bytes)
            NOT BETWEEN 32 AND 262144
        OR NOT pg_catalog.pg_input_is_valid(
            proposed_attestation_record_bytes,
            'jsonb'
        )
        OR requested_serving_lease_milliseconds
            NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_commit_input_invalid';
    END IF;

    IF proposed_attestation_record_bytes::JSONB
        IS DISTINCT FROM proposed_attestation_record
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_commit_input_invalid';
    END IF;

    SELECT pg_catalog.count(*)
    INTO record_key_count
    FROM pg_catalog.jsonb_object_keys(proposed_attestation_record);
    IF record_key_count <> 6
        OR NOT proposed_attestation_record ?& ARRAY[
            'live',
            'runtime_build_revision',
            'panel_report_digest',
            'gateway_shard_id',
            'controller_fencing_token',
            'deployment_revision'
        ]
        OR pg_catalog.jsonb_typeof(proposed_attestation_record -> 'live')
            <> 'object'
        OR proposed_attestation_record ->> 'runtime_build_revision'
            IS DISTINCT FROM expected_runtime_build_revision
        OR proposed_attestation_record ->> 'panel_report_digest'
            IS DISTINCT FROM expected_panel_report_digest
        OR proposed_attestation_record ->> 'gateway_shard_id'
            IS DISTINCT FROM expected_gateway_shard_id
        OR proposed_attestation_record ->> 'controller_fencing_token'
            IS DISTINCT FROM expected_controller_fencing_token::TEXT
        OR proposed_attestation_record ->> 'deployment_revision'
            IS DISTINCT FROM (expected_deployment_revision + 1)::TEXT
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_commit_record_invalid';
    END IF;

    SELECT pg_catalog.count(*)
    INTO gateway_key_count
    FROM pg_catalog.jsonb_object_keys(expected_gateway_ready);
    IF gateway_key_count <> 5
        OR NOT expected_gateway_ready ?& ARRAY[
            'target',
            'runtime_generation',
            'process_instance_id',
            'kind',
            'ready_at'
        ]
        OR pg_catalog.jsonb_typeof(expected_gateway_ready -> 'target')
            <> 'object'
        OR pg_catalog.jsonb_typeof(
            expected_gateway_ready -> 'runtime_generation'
        ) <> 'number'
        OR pg_catalog.jsonb_typeof(
            expected_gateway_ready -> 'process_instance_id'
        ) <> 'string'
        OR pg_catalog.jsonb_typeof(expected_gateway_ready -> 'kind')
            <> 'string'
        OR pg_catalog.jsonb_typeof(expected_gateway_ready -> 'ready_at')
            <> 'string'
        OR expected_gateway_ready ->> 'runtime_generation'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR expected_gateway_ready ->> 'process_instance_id'
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_ready ->> 'kind'
            NOT IN ('discord_ready', 'discord_resumed')
        OR NOT pg_catalog.pg_input_is_valid(
            expected_gateway_ready ->> 'ready_at',
            'timestamp with time zone'
        )
        OR expected_gateway_ready ->> 'ready_at'
            !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
        OR pg_catalog.jsonb_typeof(
            proposed_attestation_record #> '{live,activation}'
        ) <> 'object'
        OR pg_catalog.jsonb_typeof(
            proposed_attestation_record #> '{live,panel_certificate}'
        ) <> 'object'
        OR pg_catalog.jsonb_typeof(
            proposed_attestation_record #> '{live,gateway_ready}'
        ) <> 'object'
        OR NOT pg_catalog.pg_input_is_valid(
            proposed_attestation_record
                #>> '{live,activation,activated_at}',
            'timestamp with time zone'
        )
        OR NOT pg_catalog.pg_input_is_valid(
            proposed_attestation_record
                #>> '{live,panel_certificate,reconciled_at}',
            'timestamp with time zone'
        )
        OR NOT pg_catalog.pg_input_is_valid(
            proposed_attestation_record
                #>> '{live,gateway_ready,ready_at}',
            'timestamp with time zone'
        )
        OR NOT pg_catalog.pg_input_is_valid(
            proposed_attestation_record #>> '{live,certified_at}',
            'timestamp with time zone'
        )
        OR proposed_attestation_record
                #>> '{live,activation,activated_at}'
            !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
        OR proposed_attestation_record
                #>> '{live,panel_certificate,reconciled_at}'
            !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
        OR proposed_attestation_record
                #>> '{live,gateway_ready,ready_at}'
            !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
        OR proposed_attestation_record #>> '{live,certified_at}'
            !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{3}|[.][0-9]{6}|[.][0-9]{9})?Z$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_commit_record_invalid';
    END IF;

    activation_time := (
        proposed_attestation_record
            #>> '{live,activation,activated_at}'
    )::TIMESTAMPTZ;
    panel_time := (
        proposed_attestation_record
            #>> '{live,panel_certificate,reconciled_at}'
    )::TIMESTAMPTZ;
    gateway_time := (
        proposed_attestation_record
            #>> '{live,gateway_ready,ready_at}'
    )::TIMESTAMPTZ;
    certified_time := (
        proposed_attestation_record #>> '{live,certified_at}'
    )::TIMESTAMPTZ;

    canonical_activation_time := public.starring_canonical_json_v1(
        proposed_attestation_record
            #> '{live,activation,activated_at}'
    );
    canonical_panel_time := public.starring_canonical_json_v1(
        proposed_attestation_record
            #> '{live,panel_certificate,reconciled_at}'
    );
    canonical_gateway_time := public.starring_canonical_json_v1(
        proposed_attestation_record
            #> '{live,gateway_ready,ready_at}'
    );
    canonical_certified_time := public.starring_canonical_json_v1(
        proposed_attestation_record #> '{live,certified_at}'
    );

    canonical_target_bytes :=
        '{"guild_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,target,guild_id}'
        )
        || ',"ruleset_key":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,target,ruleset_key}'
        )
        || ',"version":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,target,version}'
        )
        || ',"content_hash":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,target,content_hash}'
        )
        || ',"binding_revision":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,target,binding_revision}'
        )
        || ',"binding_fingerprint":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,target,binding_fingerprint}'
        )
        || '}';
    canonical_activation_bytes :=
        '{"activation_request_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,activation,activation_request_id}'
        )
        || ',"target":' || canonical_target_bytes
        || ',"runtime_generation":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,activation,runtime_generation}'
        )
        || ',"kind":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,activation,kind}'
        )
        || ',"activated_at":' || canonical_activation_time
        || '}';
    canonical_panel_bytes :=
        '{"certificate_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,certificate_id}'
        )
        || ',"report_digest":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,report_digest}'
        )
        || ',"target":' || canonical_target_bytes
        || ',"runtime_generation":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,runtime_generation}'
        )
        || ',"process_instance_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,process_instance_id}'
        )
        || ',"declared_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,declared_count}'
        )
        || ',"installed_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,installed_count}'
        )
        || ',"unchanged_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,unchanged_count}'
        )
        || ',"skipped_transient_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,skipped_transient_count}'
        )
        || ',"skipped_unresolved_channel_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,skipped_unresolved_channel_count}'
        )
        || ',"failed_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,failed_count}'
        )
        || ',"ambiguous_outcome_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,ambiguous_outcome_count}'
        )
        || ',"stale_message_cleanup_pending_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,stale_message_cleanup_pending_count}'
        )
        || ',"orphan_message_cleanup_pending_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,orphan_message_cleanup_pending_count}'
        )
        || ',"reposted_old_message_cleanup_pending_count":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,panel_certificate,reposted_old_message_cleanup_pending_count}'
        )
        || ',"reconciled_at":' || canonical_panel_time
        || '}';
    canonical_gateway_bytes :=
        '{"target":' || canonical_target_bytes
        || ',"runtime_generation":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,gateway_ready,runtime_generation}'
        )
        || ',"process_instance_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record
                #> '{live,gateway_ready,process_instance_id}'
        )
        || ',"kind":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,gateway_ready,kind}'
        )
        || ',"ready_at":' || canonical_gateway_time
        || '}';
    canonical_live_bytes :=
        '{"target":' || canonical_target_bytes
        || ',"runtime_generation":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,runtime_generation}'
        )
        || ',"process_instance_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record #> '{live,process_instance_id}'
        )
        || ',"activation":' || canonical_activation_bytes
        || ',"panel_certificate":' || canonical_panel_bytes
        || ',"gateway_ready":' || canonical_gateway_bytes
        || ',"certified_at":' || canonical_certified_time
        || '}';
    canonical_record_bytes :=
        '{"live":' || canonical_live_bytes
        || ',"runtime_build_revision":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record -> 'runtime_build_revision'
        )
        || ',"panel_report_digest":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record -> 'panel_report_digest'
        )
        || ',"gateway_shard_id":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record -> 'gateway_shard_id'
        )
        || ',"controller_fencing_token":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record -> 'controller_fencing_token'
        )
        || ',"deployment_revision":'
        || public.starring_canonical_json_v1(
            proposed_attestation_record -> 'deployment_revision'
        )
        || '}';

    IF canonical_record_bytes IS NULL
        OR proposed_attestation_record_bytes
            IS DISTINCT FROM canonical_record_bytes
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_commit_noncanonical_record';
    END IF;

    record_bytes := pg_catalog.convert_to(canonical_record_bytes, 'UTF8');
    domain_bytes :=
        pg_catalog.convert_to(
            'starring.runtime.live_attestation.v1',
            'UTF8'
        ) || pg_catalog.decode('00', 'hex');
    computed_attestation_id := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.int8send(
                pg_catalog.octet_length(domain_bytes)::BIGINT
            )
            || domain_bytes
            || pg_catalog.int8send(
                pg_catalog.octet_length(record_bytes)::BIGINT
            )
            || record_bytes
        ),
        'hex'
    );
    IF computed_attestation_id
        IS DISTINCT FROM proposed_attestation_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_commit_digest_mismatch';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_commit_ownership_lost';
    END IF;

    IF deployment_row.revision = expected_deployment_revision + 1
        AND deployment_row.phase = 'live'
    THEN
        PERFORM pg_catalog.pg_advisory_xact_lock(
            pg_catalog.hashtextextended(
                pg_catalog.concat(
                    'starring-runtime-serving-slot-v1:',
                    deployment_row.guild_id,
                    ':',
                    deployment_row.ruleset_key
                ),
                0
            )
        );
        SELECT lease.*
        INTO serving_row
        FROM public.runtime_serving_leases AS lease
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
        FOR UPDATE;
        serving_found := FOUND;

        SELECT attestation.*
        INTO existing_attestation
        FROM public.runtime_attestations AS attestation
        WHERE attestation.tenant_id = expected_tenant_id
            AND attestation.installation_id = expected_installation_id
            AND attestation.deployment_id = expected_deployment_id
            AND attestation.attestation_id = proposed_attestation_id
        FOR KEY SHARE;
        attestation_found := FOUND;

        IF deployment_row.last_controller_id
                IS DISTINCT FROM expected_controller_id
            OR deployment_row.last_fencing_token
                IS DISTINCT FROM expected_controller_fencing_token
            OR deployment_row.convergence_attempt_no
                IS DISTINCT FROM expected_convergence_attempt_no
            OR deployment_row.runtime_generation
                IS DISTINCT FROM expected_runtime_generation
            OR deployment_row.snapshot -> 'gateway_ready'
                IS DISTINCT FROM expected_gateway_ready
            OR deployment_row.live_attestation_id
                IS DISTINCT FROM proposed_attestation_id
            OR NOT serving_found
            OR NOT attestation_found
            OR existing_attestation.record
                IS DISTINCT FROM proposed_attestation_record
            OR existing_attestation.serving_lease_duration_nanos
                IS DISTINCT FROM
                    requested_serving_lease_milliseconds * 1000000
            OR serving_row.attestation_id
                IS DISTINCT FROM proposed_attestation_id
            OR serving_row.process_instance_id
                IS DISTINCT FROM expected_gateway_ready
                    ->> 'process_instance_id'
            OR serving_row.runtime_generation
                IS DISTINCT FROM expected_runtime_generation
            OR NOT serving_row.connected
            OR NOT serving_row.serving
            OR serving_row.expires_at <= pg_catalog.clock_timestamp()
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_certify_commit_replay_mismatch';
        END IF;

        outcome_name := 'replayed';
        previous_snapshot := deployment_row.snapshot;
        snapshot := deployment_row.snapshot;
        convergence_attempt_no := expected_convergence_attempt_no;
        tenant_id := serving_row.tenant_id;
        installation_id := serving_row.installation_id;
        deployment_id := serving_row.deployment_id;
        guild_id := serving_row.guild_id;
        ruleset_key := serving_row.ruleset_key;
        attestation_id := serving_row.attestation_id;
        process_instance_id := serving_row.process_instance_id;
        runtime_generation := serving_row.runtime_generation;
        lease_epoch := serving_row.lease_epoch;
        serving_revision := serving_row.revision;
        acquired_at := serving_row.acquired_at;
        last_heartbeat_at := serving_row.last_heartbeat_at;
        expires_at := serving_row.expires_at;
        connected := serving_row.connected;
        serving := serving_row.serving;
        RETURN NEXT;
        RETURN;
    END IF;

    IF deployment_row.revision IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.snapshot
            IS DISTINCT FROM expected_observed_snapshot
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR deployment_row.controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.controller_lease_expires_at IS NULL
        OR expected_gateway_ready -> 'target'
            IS DISTINCT FROM deployment_row.snapshot -> 'target'
        OR expected_gateway_ready ->> 'runtime_generation'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR expected_gateway_ready ->> 'process_instance_id'
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,process_instance_id}'
        OR expected_panel_report_digest
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,report_digest}'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_commit_ownership_lost';
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );
    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_certify_commit_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_certify_commit_authority_changed';
    END IF;

    SELECT version.content_hash = deployment_row.target_content_hash
        AND version.canonical_content_hash
            = deployment_row.target_content_hash
        AND version.schema_version = 1
    INTO canonical_artifact
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = deployment_row.guild_id
        AND version.ruleset_key = deployment_row.ruleset_key
        AND version.version = deployment_row.target_version
    FOR SHARE;
    IF canonical_artifact IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_commit_artifact_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                deployment_row.guild_id,
                ':',
                deployment_row.ruleset_key
            ),
            0
        )
    );
    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    serving_found := FOUND;

    current_clock := public.starring_runtime_current_mutation_clock();
    IF current_clock IS DISTINCT FROM expected_mutation_clock THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_commit_prepare_mismatch';
    END IF;
    IF deployment_row.controller_lease_expires_at <= current_clock THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_commit_lease_expired';
    END IF;
    IF gateway_time < (
            deployment_row.snapshot
                #>> '{panel_certificate,reconciled_at}'
        )::TIMESTAMPTZ
        OR gateway_time < current_clock - INTERVAL '90 seconds'
        OR gateway_time > current_clock + INTERVAL '30 seconds'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_certify_commit_gateway_invalid';
    END IF;

    IF certified_time IS DISTINCT FROM current_clock
        OR proposed_attestation_record #> '{live,target}'
            IS DISTINCT FROM deployment_row.snapshot -> 'target'
        OR proposed_attestation_record
            #>> '{live,runtime_generation}'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR proposed_attestation_record
            #>> '{live,process_instance_id}'
            IS DISTINCT FROM expected_gateway_ready
                ->> 'process_instance_id'
        OR proposed_attestation_record #> '{live,activation}'
            IS DISTINCT FROM deployment_row.snapshot -> 'activation'
        OR proposed_attestation_record #> '{live,panel_certificate}'
            IS DISTINCT FROM deployment_row.snapshot -> 'panel_certificate'
        OR proposed_attestation_record #> '{live,gateway_ready}'
            IS DISTINCT FROM expected_gateway_ready
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_commit_live_mismatch';
    END IF;

    live_value := pg_catalog.jsonb_build_object(
        'target', deployment_row.snapshot -> 'target',
        'runtime_generation', expected_runtime_generation,
        'process_instance_id',
            expected_gateway_ready ->> 'process_instance_id',
        'activation', deployment_row.snapshot -> 'activation',
        'panel_certificate', deployment_row.snapshot -> 'panel_certificate',
        'gateway_ready', expected_gateway_ready,
        'certified_at',
            proposed_attestation_record #> '{live,certified_at}'
    );
    expected_record := pg_catalog.jsonb_build_object(
        'live', live_value,
        'runtime_build_revision', expected_runtime_build_revision,
        'panel_report_digest', expected_panel_report_digest,
        'gateway_shard_id', expected_gateway_shard_id,
        'controller_fencing_token', expected_controller_fencing_token,
        'deployment_revision', expected_deployment_revision + 1
    );
    IF expected_record IS DISTINCT FROM proposed_attestation_record THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_certify_commit_record_mismatch';
    END IF;

    IF serving_found
        AND serving_row.expires_at > current_clock
        AND (serving_row.connected OR serving_row.serving)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_commit_serving_conflict';
    END IF;

    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(expected_deployment_revision + 1),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{phase}',
        '{"phase":"live"}'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{controller_lease}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{gateway_ready}',
        expected_gateway_ready,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{live}',
        live_value,
        FALSE
    );

    INSERT INTO public.runtime_attestations (
        attestation_id,
        attestation_digest,
        deployment_id,
        deployment_revision,
        convergence_attempt_no,
        serving_lease_duration_nanos,
        tenant_id,
        installation_id,
        promotion_id,
        activation_request_id,
        guild_id,
        ruleset_key,
        target_version,
        target_content_hash,
        binding_revision,
        binding_fingerprint,
        runtime_generation,
        controller_fencing_token,
        process_instance_id,
        runtime_build_revision,
        panel_certificate_id,
        panel_report_digest,
        gateway_shard_id,
        gateway_ready_kind,
        gateway_ready_at,
        certified_at,
        record_format_version,
        record,
        created_at
    )
    VALUES (
        proposed_attestation_id,
        proposed_attestation_id,
        expected_deployment_id,
        expected_deployment_revision + 1,
        expected_convergence_attempt_no,
        requested_serving_lease_milliseconds * 1000000,
        expected_tenant_id,
        expected_installation_id,
        deployment_row.promotion_id,
        deployment_row.activation_request_id,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint,
        expected_runtime_generation,
        expected_controller_fencing_token,
        expected_gateway_ready ->> 'process_instance_id',
        expected_runtime_build_revision,
        deployment_row.snapshot
            #>> '{panel_certificate,certificate_id}',
        expected_panel_report_digest,
        expected_gateway_shard_id,
        expected_gateway_ready ->> 'kind',
        (expected_gateway_ready ->> 'ready_at')::TIMESTAMPTZ,
        current_clock,
        1,
        proposed_attestation_record,
        current_clock
    );

    requested_duration :=
        requested_serving_lease_milliseconds * INTERVAL '1 millisecond';
    next_expiry := current_clock + requested_duration;
    IF serving_row.guild_id IS NULL THEN
        next_epoch := 1;
        next_serving_revision := 1;
        INSERT INTO public.runtime_serving_leases (
            guild_id,
            ruleset_key,
            tenant_id,
            installation_id,
            deployment_id,
            attestation_id,
            process_instance_id,
            runtime_generation,
            target_version,
            target_content_hash,
            binding_revision,
            binding_fingerprint,
            lease_epoch,
            revision,
            connected,
            serving,
            acquired_at,
            last_heartbeat_at,
            expires_at
        )
        VALUES (
            deployment_row.guild_id,
            deployment_row.ruleset_key,
            expected_tenant_id,
            expected_installation_id,
            expected_deployment_id,
            proposed_attestation_id,
            expected_gateway_ready ->> 'process_instance_id',
            expected_runtime_generation,
            deployment_row.target_version,
            deployment_row.target_content_hash,
            deployment_row.binding_revision,
            deployment_row.binding_fingerprint,
            next_epoch,
            next_serving_revision,
            TRUE,
            TRUE,
            current_clock,
            current_clock,
            next_expiry
        )
        RETURNING * INTO serving_row;
    ELSE
        IF serving_row.lease_epoch = 9223372036854775807
            OR serving_row.revision = 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_certify_commit_serving_overflow';
        END IF;
        next_epoch := serving_row.lease_epoch + 1;
        next_serving_revision := serving_row.revision + 1;
        UPDATE public.runtime_serving_leases AS lease
        SET tenant_id = expected_tenant_id,
            installation_id = expected_installation_id,
            deployment_id = expected_deployment_id,
            attestation_id = proposed_attestation_id,
            process_instance_id =
                expected_gateway_ready ->> 'process_instance_id',
            runtime_generation = expected_runtime_generation,
            target_version = deployment_row.target_version,
            target_content_hash = deployment_row.target_content_hash,
            binding_revision = deployment_row.binding_revision,
            binding_fingerprint = deployment_row.binding_fingerprint,
            lease_epoch = next_epoch,
            revision = next_serving_revision,
            connected = TRUE,
            serving = TRUE,
            acquired_at = current_clock,
            last_heartbeat_at = current_clock,
            expires_at = next_expiry
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
        RETURNING * INTO serving_row;
    END IF;

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = expected_deployment_revision + 1,
        phase = 'live',
        controller_id = NULL,
        controller_fencing_token = NULL,
        controller_acquired_at = NULL,
        controller_lease_expires_at = NULL,
        live_attestation_id = proposed_attestation_id,
        live_at = current_clock,
        last_controller_id = expected_controller_id,
        updated_at = GREATEST(
            current_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
        AND deployment.revision = expected_deployment_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_certify_commit_ownership_lost';
    END IF;

    outcome_name := 'applied';
    previous_snapshot := expected_observed_snapshot;
    snapshot := next_snapshot;
    convergence_attempt_no := expected_convergence_attempt_no;
    tenant_id := serving_row.tenant_id;
    installation_id := serving_row.installation_id;
    deployment_id := serving_row.deployment_id;
    guild_id := serving_row.guild_id;
    ruleset_key := serving_row.ruleset_key;
    attestation_id := serving_row.attestation_id;
    process_instance_id := serving_row.process_instance_id;
    runtime_generation := serving_row.runtime_generation;
    lease_epoch := serving_row.lease_epoch;
    serving_revision := serving_row.revision;
    acquired_at := serving_row.acquired_at;
    last_heartbeat_at := serving_row.last_heartbeat_at;
    expires_at := serving_row.expires_at;
    connected := serving_row.connected;
    serving := serving_row.serving;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_execution_recover_stale_live_v1()
RETURNS TABLE(
    outcome_name TEXT,
    observed_snapshot JSONB,
    deployment_snapshot JSONB,
    convergence_attempt_no BIGINT,
    loss_kind TEXT,
    evidence_at TIMESTAMPTZ,
    recovered_at TIMESTAMPTZ
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
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    previous_snapshot JSONB;
    next_snapshot JSONB;
    recovery_value JSONB;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    recovery_kind TEXT;
    recovery_evidence TIMESTAMPTZ;
    next_revision BIGINT;
    serving_found BOOLEAN;
BEGIN
    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    JOIN public.runtime_serving_leases AS serving_lease
        ON serving_lease.guild_id = deployment.guild_id
        AND serving_lease.ruleset_key = deployment.ruleset_key
        AND serving_lease.tenant_id = deployment.tenant_id
        AND serving_lease.installation_id = deployment.installation_id
        AND serving_lease.deployment_id = deployment.deployment_id
        AND serving_lease.attestation_id = deployment.live_attestation_id
    JOIN public.activation_requests AS activation
        ON activation.id = deployment.activation_request_id
        AND activation.state = 'applied'
        AND activation.authority_kind = 'product_authoring'
        AND activation.link_state_name = 'linked'
        AND activation.promotion_id = deployment.promotion_id
    JOIN public.authoring_promotions AS promotion
        ON promotion.id = deployment.promotion_id
        AND promotion.stage = 'activation_pending'
        AND promotion.tenant_id = deployment.tenant_id
    JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = deployment.tenant_id
        AND tenant.lifecycle_state = 'active'
    JOIN public.automation_installations AS installation
        ON installation.tenant_id = deployment.tenant_id
        AND installation.installation_id = deployment.installation_id
        AND installation.lifecycle_state = 'active'
    JOIN public.automation_installation_authority_versions
        AS historical_authority
        ON historical_authority.tenant_id = installation.tenant_id
        AND historical_authority.installation_id
            = installation.installation_id
        AND historical_authority.revision
            = deployment.installation_authority_revision
        AND historical_authority.binding_revision
            = deployment.binding_revision
        AND historical_authority.binding_fingerprint
            = deployment.binding_fingerprint
    JOIN public.automation_installation_authority_versions
        AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id
            = installation.installation_id
        AND current_authority.revision
            = installation.current_authority_revision
        AND current_authority.binding_revision
            = deployment.binding_revision
        AND current_authority.binding_fingerprint
            = deployment.binding_fingerprint
        AND current_authority.resource_bindings
            IS NOT DISTINCT FROM historical_authority.resource_bindings
    JOIN public.automation_ruleset_activations AS active
        ON active.guild_id = deployment.guild_id
        AND active.ruleset_key = deployment.ruleset_key
        AND active.active_version = deployment.target_version
    JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
        AND version.content_hash = deployment.target_content_hash
        AND version.canonical_content_hash = version.content_hash
        AND version.schema_version = 1
    WHERE deployment.phase = 'live'
        AND deployment.revision < 9223372036854775807
        AND deployment.convergence_attempt_no BETWEEN 1 AND 4294967295
        AND serving_lease.process_instance_id
            = deployment.snapshot #>> '{live,process_instance_id}'
        AND serving_lease.runtime_generation
            = deployment.runtime_generation
        AND (
            NOT serving_lease.connected
            OR NOT serving_lease.serving
            OR serving_lease.expires_at <= pg_catalog.clock_timestamp()
        )
        AND promotion.record #>> '{intent,authority,tenant_id}'
            = deployment.tenant_id
        AND promotion.record #>> '{intent,authority,installation_id}'
            = deployment.installation_id
        AND promotion.record #>> '{intent,authority,guild_id}'
            = deployment.guild_id
        AND promotion.record #>> '{intent,authority,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record #>> '{intent,authority,binding_revision}'
            = deployment.binding_revision::TEXT
        AND promotion.record #>> '{intent,evidence,context_fingerprint}'
            = deployment.binding_fingerprint
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS newer
            WHERE newer.guild_id = deployment.guild_id
                AND newer.ruleset_key = deployment.ruleset_key
                AND newer.deployment_id <> deployment.deployment_id
                AND newer.phase NOT IN ('live', 'superseded', 'cancelled')
        )
    ORDER BY
        serving_lease.expires_at,
        deployment.updated_at,
        deployment.deployment_id
    LIMIT 1
    FOR UPDATE OF deployment SKIP LOCKED;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );
    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_recover_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_recover_authority_changed';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                deployment_row.guild_id,
                ':',
                deployment_row.ruleset_key
            ),
            0
        )
    );
    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    serving_found := FOUND;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF NOT serving_found
        OR serving_row.tenant_id
            IS DISTINCT FROM deployment_row.tenant_id
        OR serving_row.installation_id
            IS DISTINCT FROM deployment_row.installation_id
        OR serving_row.deployment_id
            IS DISTINCT FROM deployment_row.deployment_id
        OR serving_row.attestation_id
            IS DISTINCT FROM deployment_row.live_attestation_id
        OR serving_row.process_instance_id
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{live,process_instance_id}'
        OR serving_row.runtime_generation
            IS DISTINCT FROM deployment_row.runtime_generation
        OR serving_row.guild_id IS DISTINCT FROM deployment_row.guild_id
        OR serving_row.ruleset_key
            IS DISTINCT FROM deployment_row.ruleset_key
        OR serving_row.target_version
            IS DISTINCT FROM deployment_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.acquired_at > mutation_clock
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_recover_state_invalid';
    END IF;

    IF NOT serving_row.connected AND NOT serving_row.serving
        AND serving_row.last_heartbeat_at = serving_row.expires_at
        AND serving_row.expires_at <= mutation_clock
    THEN
        recovery_kind := 'serving_disconnected';
        recovery_evidence := serving_row.last_heartbeat_at;
    ELSIF serving_row.connected AND serving_row.serving
        AND serving_row.last_heartbeat_at < serving_row.expires_at
        AND serving_row.expires_at <= mutation_clock
    THEN
        recovery_kind := 'serving_lease_expired';
        recovery_evidence := serving_row.expires_at;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_recover_serving_active';
    END IF;

    IF recovery_evidence
            < (deployment_row.snapshot
                #>> '{live,certified_at}')::TIMESTAMPTZ
        OR EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS newer
            WHERE newer.guild_id = deployment_row.guild_id
                AND newer.ruleset_key = deployment_row.ruleset_key
                AND newer.deployment_id <> deployment_row.deployment_id
                AND newer.phase NOT IN ('live', 'superseded', 'cancelled')
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_recover_ownership_lost';
    END IF;

    previous_snapshot := deployment_row.snapshot;
    next_revision := deployment_row.revision + 1;
    recovery_value := pg_catalog.jsonb_build_object(
        'prior_live', deployment_row.snapshot -> 'live',
        'kind', recovery_kind,
        'evidence_at', recovery_evidence,
        'recovered_at', mutation_clock
    );
    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{phase}',
        '{"phase":"runtime_pending","condition":{"condition":"ready"}}'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{panel_certificate}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{gateway_ready}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{live}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{last_live_recovery}',
        recovery_value,
        FALSE
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        phase = 'runtime_pending',
        live_attestation_id = NULL,
        live_at = NULL,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = deployment_row.tenant_id
        AND deployment.installation_id = deployment_row.installation_id
        AND deployment.deployment_id = deployment_row.deployment_id
        AND deployment.revision = deployment_row.revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_recover_ownership_lost';
    END IF;

    outcome_name := 'applied';
    observed_snapshot := previous_snapshot;
    deployment_snapshot := next_snapshot;
    convergence_attempt_no := deployment_row.convergence_attempt_no;
    loss_kind := recovery_kind;
    evidence_at := recovery_evidence;
    recovered_at := mutation_clock;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_execution_schema_manifest_v1()
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
    WITH protected(relation_oid) AS (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_execution_mutation_markers')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ), protected_function(function_oid) AS (
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_claim_next_v1(text,bigint)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_recover_stale_live_v1()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_mutation_clock()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_current_mutation_clock()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_canonical_json_v1(jsonb)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_ruleset_content_hash_v1(bigint,jsonb)'
        )
        UNION
        SELECT trigger_row.tgfoid
        FROM pg_catalog.pg_trigger AS trigger_row
        WHERE trigger_row.tgrelid IN (SELECT relation_oid FROM protected)
            AND NOT trigger_row.tgisinternal
    ), manifest(value) AS (
        SELECT pg_catalog.concat_ws(
            '|',
            'relation',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            relation.relkind::TEXT,
            relation.relpersistence::TEXT,
            relation.relispartition::TEXT,
            relation.relrowsecurity::TEXT,
            relation.relforcerowsecurity::TEXT,
            relation.relreplident::TEXT
        )
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE relation.oid IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'inheritance',
            pg_catalog.format(
                '%I.%I',
                child_namespace.nspname,
                child.relname
            ),
            pg_catalog.format(
                '%I.%I',
                parent_namespace.nspname,
                parent.relname
            ),
            inheritance.inhseqno::TEXT,
            inheritance.inhdetachpending::TEXT
        )
        FROM pg_catalog.pg_inherits AS inheritance
        INNER JOIN pg_catalog.pg_class AS child
            ON child.oid = inheritance.inhrelid
        INNER JOIN pg_catalog.pg_namespace AS child_namespace
            ON child_namespace.oid = child.relnamespace
        INNER JOIN pg_catalog.pg_class AS parent
            ON parent.oid = inheritance.inhparent
        INNER JOIN pg_catalog.pg_namespace AS parent_namespace
            ON parent_namespace.oid = parent.relnamespace
        WHERE inheritance.inhrelid IN (SELECT relation_oid FROM protected)
            OR inheritance.inhparent IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'attribute',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            attribute.attnum::TEXT,
            attribute.attname,
            pg_catalog.format_type(attribute.atttypid, attribute.atttypmod),
            attribute.attnotnull::TEXT,
            attribute.attidentity::TEXT,
            attribute.attgenerated::TEXT,
            attribute.attstorage::TEXT,
            attribute.attcompression::TEXT,
            attribute.atthasdef::TEXT,
            COALESCE(
                pg_catalog.pg_get_expr(default_row.adbin, default_row.adrelid),
                ''
            ),
            COALESCE(collation_namespace.nspname, ''),
            COALESCE(collation_row.collname, '')
        )
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        INNER JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = relation.oid
            AND attribute.attnum > 0
            AND NOT attribute.attisdropped
        LEFT JOIN pg_catalog.pg_attrdef AS default_row
            ON default_row.adrelid = attribute.attrelid
            AND default_row.adnum = attribute.attnum
        LEFT JOIN pg_catalog.pg_collation AS collation_row
            ON collation_row.oid = attribute.attcollation
        LEFT JOIN pg_catalog.pg_namespace AS collation_namespace
            ON collation_namespace.oid = collation_row.collnamespace
        WHERE relation.oid IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'constraint',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            constraint_row.conname,
            constraint_row.contype::TEXT,
            constraint_row.convalidated::TEXT,
            constraint_row.condeferrable::TEXT,
            constraint_row.condeferred::TEXT,
            constraint_row.connoinherit::TEXT,
            constraint_row.conislocal::TEXT,
            constraint_row.coninhcount::TEXT,
            (constraint_row.conparentid = 0)::TEXT,
            COALESCE(index_row.relname, ''),
            pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE)
        )
        FROM pg_catalog.pg_constraint AS constraint_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = constraint_row.conrelid
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        LEFT JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = constraint_row.conindid
        WHERE constraint_row.conrelid IN (SELECT relation_oid FROM protected)
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
        WHERE index_contract.indrelid IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'trigger',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            trigger_row.tgname,
            pg_catalog.format(
                '%I.%I(%s)',
                function_namespace.nspname,
                function_row.proname,
                pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            ),
            trigger_row.tgtype::TEXT,
            trigger_row.tgenabled::TEXT,
            trigger_row.tgisinternal::TEXT,
            trigger_row.tgnargs::TEXT,
            pg_catalog.octet_length(trigger_row.tgargs)::TEXT,
            trigger_row.tgattr::TEXT,
            (trigger_row.tgqual IS NULL)::TEXT,
            (trigger_row.tgconstraint = 0)::TEXT,
            trigger_row.tgdeferrable::TEXT,
            trigger_row.tginitdeferred::TEXT,
            COALESCE(trigger_row.tgoldtable, ''),
            COALESCE(trigger_row.tgnewtable, ''),
            pg_catalog.pg_get_triggerdef(trigger_row.oid, TRUE)
        )
        FROM pg_catalog.pg_trigger AS trigger_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = trigger_row.tgrelid
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = trigger_row.tgfoid
        INNER JOIN pg_catalog.pg_namespace AS function_namespace
            ON function_namespace.oid = function_row.pronamespace
        WHERE trigger_row.tgrelid IN (SELECT relation_oid FROM protected)
            AND NOT trigger_row.tgisinternal
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'function',
            pg_catalog.format(
                '%I.%I(%s)',
                namespace.nspname,
                function_row.proname,
                pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            ),
            language_row.lanname,
            pg_catalog.pg_get_function_result(function_row.oid),
            pg_catalog.pg_get_functiondef(function_row.oid)
        )
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        INNER JOIN pg_catalog.pg_language AS language_row
            ON language_row.oid = function_row.prolang
        WHERE function_row.oid IN (
            SELECT function_oid
            FROM protected_function
            WHERE function_oid IS NOT NULL
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

    RETURN observed_count = 472
        AND observed_digest
            = '3d12dc4468b6d42cd9ec0b5bc0814117684fff43e28356f1fb40089c127ab641';
END;
$function$;

DO $privileges$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
    column_name NAME;
    default_grantee_clause TEXT;
    default_schema_name NAME;
    relation_identity TEXT;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);

    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_owner_drift';
    END IF;

    EXECUTE pg_catalog.format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC',
        common_owner_name
    );

    FOR default_schema_name, grantee IN
        SELECT namespace.nspname, privilege.grantee
        FROM pg_catalog.pg_default_acl AS default_acl
        CROSS JOIN LATERAL
            pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
        LEFT JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = default_acl.defaclnamespace
        WHERE default_acl.defaclrole = common_owner
            AND default_acl.defaclobjtype = 'f'
            AND privilege.grantee <> common_owner
            AND (
                default_acl.defaclnamespace = 0
                OR (
                    namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                )
            )
        ORDER BY namespace.nspname NULLS FIRST, privilege.grantee
    LOOP
        default_grantee_clause := CASE
            WHEN grantee = 0 THEN 'PUBLIC'
            ELSE pg_catalog.quote_ident(pg_catalog.pg_get_userbyid(grantee))
        END;
        IF default_grantee_clause IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_execution_database_default_grantee_drift';
        END IF;
        IF default_schema_name IS NULL THEN
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s',
                common_owner_name,
                default_grantee_clause
            );
        ELSE
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s',
                common_owner_name,
                default_schema_name,
                default_grantee_clause
            );
        END IF;
    END LOOP;

    EXECUTE pg_catalog.format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %I',
        common_owner_name,
        common_owner_name
    );
    EXECUTE pg_catalog.format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I GRANT EXECUTE ON FUNCTIONS TO %I',
        common_owner_name,
        common_owner_name
    );

    FOREACH relation_identity IN ARRAY ARRAY[
        'public.product_control_plane_identity',
        'public.runtime_deployments',
        'public.runtime_execution_mutation_markers',
        'public.runtime_attestations',
        'public.runtime_serving_leases',
        'public.activation_requests',
        'public.authoring_promotions',
        'public.product_tenants',
        'public.automation_installations',
        'public.automation_installation_authority_versions',
        'public.automation_ruleset_activations',
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
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_execution_database_relation_grantee_drift';
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
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_execution_database_column_grantee_drift';
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
        'public.starring_runtime_execution_schema_manifest_v1()',
        'public.starring_runtime_execution_database_identity_v1()',
        'public.starring_runtime_execution_claim_next_v1(TEXT,BIGINT)',
        'public.starring_runtime_execution_renew_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)',
        'public.starring_runtime_execution_mutate_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,JSONB)',
        'public.starring_runtime_execution_certify_prepare_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,JSONB,TEXT,TEXT,TEXT,BIGINT)',
        'public.starring_runtime_execution_certify_commit_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,JSONB,TEXT,TEXT,TEXT,BIGINT,TIMESTAMPTZ,JSONB,TEXT,JSONB,TEXT)',
        'public.starring_runtime_execution_recover_stale_live_v1()',
        'public.starring_runtime_observe_previous_serving_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,JSONB)',
        'public.starring_runtime_lock_current_authority(TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT)',
        'public.starring_runtime_mutation_clock()',
        'public.starring_runtime_current_mutation_clock()',
        'public.starring_canonical_json_v1(JSONB)',
        'public.starring_ruleset_content_hash_v1(BIGINT,JSONB)',
        'public.validate_runtime_deployment_projection()',
        'public.enforce_runtime_deployment_policy_shadow()',
        'public.guard_runtime_ruleset_artifact_transition()',
        'public.reject_runtime_deployment_delete()',
        'public.validate_runtime_attestation_projection()',
        'public.reject_immutable_product_row()',
        'public.validate_runtime_serving_lease_transition()',
        'public.reject_runtime_serving_lease_delete()',
        'public.validate_runtime_execution_mutation_marker_transition()',
        'public.reject_runtime_execution_mutation_marker_delete()',
        'public.reject_ruleset_artifact_mutation()'
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
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_execution_database_function_grantee_drift';
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
    invalid_function_count BIGINT;
    invalid_support_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.runtime_deployments'),
            ('public.runtime_execution_mutation_markers'),
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases'),
            ('public.activation_requests'),
            ('public.authoring_promotions'),
            ('public.product_tenants'),
            ('public.automation_installations'),
            ('public.automation_installation_authority_versions'),
            ('public.automation_ruleset_activations'),
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
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                ''::TEXT,
                'boolean'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_execution_database_identity_v1()',
                ''::TEXT,
                'text'::TEXT,
                'sql'::TEXT,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_execution_claim_next_v1(text,bigint)',
                'expected_controller_id text, requested_lease_milliseconds bigint'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, controller_id text, fencing_token bigint, previous_convergence_attempt_no bigint, convergence_attempt_no bigint, acquired_at timestamp with time zone, expires_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, requested_lease_milliseconds bigint'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, controller_id text, fencing_token bigint, convergence_attempt_no bigint, acquired_at timestamp with time zone, expires_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, mutation_kind text, mutation_payload jsonb'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, convergence_attempt_no bigint, mutated_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_gateway_ready jsonb, expected_runtime_build_revision text, expected_panel_report_digest text, expected_gateway_shard_id text, requested_serving_lease_milliseconds bigint'::TEXT,
                'TABLE(preparation_name text, observed_snapshot jsonb, convergence_attempt_no bigint, mutation_clock timestamp with time zone, certified_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_gateway_ready jsonb, expected_runtime_build_revision text, expected_panel_report_digest text, expected_gateway_shard_id text, requested_serving_lease_milliseconds bigint, expected_mutation_clock timestamp with time zone, expected_observed_snapshot jsonb, proposed_attestation_id text, proposed_attestation_record jsonb, proposed_attestation_record_bytes text'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, convergence_attempt_no bigint, tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, attestation_id text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, serving_revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_recover_stale_live_v1()',
                ''::TEXT,
                'TABLE(outcome_name text, observed_snapshot jsonb, deployment_snapshot jsonb, convergence_attempt_no bigint, loss_kind text, evidence_at timestamp with time zone, recovered_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text, expected_previous_runtime jsonb'::TEXT,
                'TABLE(state_name text, observed_at timestamp with time zone, lease_tenant_id text, lease_installation_id text, lease_deployment_id text, lease_attestation_id text, lease_process_instance_id text, lease_runtime_generation bigint, lease_guild_id text, lease_ruleset_key text, lease_target_version bigint, lease_target_content_hash text, lease_binding_revision bigint, lease_binding_fingerprint text, lease_epoch bigint, lease_revision bigint, lease_connected boolean, lease_serving boolean, lease_acquired_at timestamp with time zone, lease_last_heartbeat_at timestamp with time zone, lease_expires_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                FALSE,
                TRUE,
                1::REAL
            )
    ) AS expected(
        identity,
        arguments,
        result,
        language_name,
        is_strict,
        returns_set,
        rows_estimate
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
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
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
    INTO invalid_support_acl_count
    FROM (
        VALUES
            ('public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'),
            ('public.starring_runtime_mutation_clock()'),
            ('public.starring_runtime_current_mutation_clock()'),
            ('public.starring_canonical_json_v1(jsonb)'),
            ('public.starring_ruleset_content_hash_v1(bigint,jsonb)'),
            ('public.validate_runtime_deployment_projection()'),
            ('public.enforce_runtime_deployment_policy_shadow()'),
            ('public.guard_runtime_ruleset_artifact_transition()'),
            ('public.reject_runtime_deployment_delete()'),
            ('public.validate_runtime_attestation_projection()'),
            ('public.reject_immutable_product_row()'),
            ('public.validate_runtime_serving_lease_transition()'),
            ('public.reject_runtime_serving_lease_delete()'),
            ('public.validate_runtime_execution_mutation_marker_transition()'),
            ('public.reject_runtime_execution_mutation_marker_delete()'),
            ('public.reject_ruleset_artifact_mutation()')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
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

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_support_acl_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_postflight_drift';
    END IF;
END;
$postflight$;

CREATE FUNCTION public.starring_runtime_execution_database_readiness_v1()
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
    invalid_function_count BIGINT;
    invalid_support_function_count BIGINT;
    invalid_protected_function_count BIGINT;
    identity_count BIGINT;
    unexpected_capability_count BIGINT;
    unsafe_schema_count BIGINT;
    unsafe_default_count BIGINT;
    owner_function_default_count BIGINT;
    invalid_owner_function_default_count BIGINT;
    unsafe_system_count BIGINT;
    role_found BOOLEAN;
    role_row RECORD;
BEGIN
    IF pg_catalog.current_setting('role') <> 'none' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_role_drift';
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
        role.rolconfig,
        role.rolname
    INTO role_row
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = invoker_oid;
    role_found := FOUND;

    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

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
        OR role_row.rolname::TEXT !~ '^[a-z_][a-z0-9_]{0,62}$'
        OR pg_catalog.current_database() !~ '^[a-z_][a-z0-9_]{0,62}$'
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
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_role_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.runtime_deployments'),
            ('public.runtime_execution_mutation_markers'),
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases'),
            ('public.activation_requests'),
            ('public.authoring_promotions'),
            ('public.product_tenants'),
            ('public.automation_installations'),
            ('public.automation_installation_authority_versions'),
            ('public.automation_ruleset_activations'),
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

    IF invalid_relation_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_schema_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                ''::TEXT,
                'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_database_identity_v1()',
                ''::TEXT,
                'text'::TEXT,
                'sql'::TEXT,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_execution_claim_next_v1(text,bigint)',
                'expected_controller_id text, requested_lease_milliseconds bigint'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, controller_id text, fencing_token bigint, previous_convergence_attempt_no bigint, convergence_attempt_no bigint, acquired_at timestamp with time zone, expires_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, requested_lease_milliseconds bigint'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, controller_id text, fencing_token bigint, convergence_attempt_no bigint, acquired_at timestamp with time zone, expires_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, mutation_kind text, mutation_payload jsonb'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, convergence_attempt_no bigint, mutated_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_gateway_ready jsonb, expected_runtime_build_revision text, expected_panel_report_digest text, expected_gateway_shard_id text, requested_serving_lease_milliseconds bigint'::TEXT,
                'TABLE(preparation_name text, observed_snapshot jsonb, convergence_attempt_no bigint, mutation_clock timestamp with time zone, certified_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_gateway_ready jsonb, expected_runtime_build_revision text, expected_panel_report_digest text, expected_gateway_shard_id text, requested_serving_lease_milliseconds bigint, expected_mutation_clock timestamp with time zone, expected_observed_snapshot jsonb, proposed_attestation_id text, proposed_attestation_record jsonb, proposed_attestation_record_bytes text'::TEXT,
                'TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, convergence_attempt_no bigint, tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, attestation_id text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, serving_revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_execution_recover_stale_live_v1()',
                ''::TEXT,
                'TABLE(outcome_name text, observed_snapshot jsonb, deployment_snapshot jsonb, convergence_attempt_no bigint, loss_kind text, evidence_at timestamp with time zone, recovered_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text, expected_previous_runtime jsonb'::TEXT,
                'TABLE(state_name text, observed_at timestamp with time zone, lease_tenant_id text, lease_installation_id text, lease_deployment_id text, lease_attestation_id text, lease_process_instance_id text, lease_runtime_generation bigint, lease_guild_id text, lease_ruleset_key text, lease_target_version bigint, lease_target_content_hash text, lease_binding_revision bigint, lease_binding_fingerprint text, lease_epoch bigint, lease_revision bigint, lease_connected boolean, lease_serving boolean, lease_acquired_at timestamp with time zone, lease_last_heartbeat_at timestamp with time zone, lease_expires_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                FALSE,
                TRUE,
                1::REAL
            )
    ) AS expected(
        identity,
        arguments,
        result,
        language_name,
        is_strict,
        returns_set,
        rows_estimate
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
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR NOT pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, invoker_oid)
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_support_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                ''::TEXT,
                'boolean'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                '242c36e163845f1b5b13f09b82676ce1af39a86214eab5b2f88143ae9c386940'::TEXT
            )
    ) AS expected(
        identity,
        arguments,
        result,
        language_name,
        is_strict,
        definition_digest
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
        OR function_row.proretset
        OR function_row.prorows <> 0::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
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
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_support_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_protected_function_count
    FROM (
        VALUES
            ('public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'),
            ('public.starring_runtime_mutation_clock()'),
            ('public.starring_runtime_current_mutation_clock()'),
            ('public.starring_canonical_json_v1(jsonb)'),
            ('public.starring_ruleset_content_hash_v1(bigint,jsonb)'),
            ('public.validate_runtime_deployment_projection()'),
            ('public.enforce_runtime_deployment_policy_shadow()'),
            ('public.guard_runtime_ruleset_artifact_transition()'),
            ('public.reject_runtime_deployment_delete()'),
            ('public.validate_runtime_attestation_projection()'),
            ('public.reject_immutable_product_row()'),
            ('public.validate_runtime_serving_lease_transition()'),
            ('public.reject_runtime_serving_lease_delete()'),
            ('public.validate_runtime_execution_mutation_marker_transition()'),
            ('public.reject_runtime_execution_mutation_marker_delete()'),
            ('public.reject_ruleset_artifact_mutation()')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
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

    IF invalid_protected_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_protected_function_drift';
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
    WHERE privilege.grantee IN (0, invoker_oid)
        OR (
            defaults.defaclrole = common_owner
            AND defaults.defaclobjtype = 'f'
            AND privilege.grantee <> common_owner
        );

    SELECT pg_catalog.count(*)
    INTO owner_function_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    WHERE defaults.defaclrole = common_owner
        AND defaults.defaclnamespace = 0
        AND defaults.defaclobjtype = 'f';

    SELECT pg_catalog.count(*)
    INTO invalid_owner_function_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL (
        SELECT pg_catalog.count(*) AS privilege_count,
            pg_catalog.count(*) FILTER (
                WHERE privilege.grantor <> common_owner
                    OR privilege.grantee <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
            ) AS invalid_privilege_count
        FROM pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    ) AS summary
    WHERE defaults.defaclrole = common_owner
        AND defaults.defaclnamespace = 0
        AND defaults.defaclobjtype = 'f'
        AND (
            summary.privilege_count <> 1
            OR summary.invalid_privilege_count <> 0
        );

    SELECT pg_catalog.count(*)
    INTO unexpected_capability_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE function_row.oid >= 16384
        AND pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        AND function_row.oid NOT IN (
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_database_readiness_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_database_identity_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_claim_next_v1(text,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_execution_recover_stale_live_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'
            )
        )
        AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_';

    IF unexpected_capability_count <> 0
        OR unsafe_schema_count <> 0
        OR unsafe_default_count <> 0
        OR owner_function_default_count <> 1
        OR invalid_owner_function_default_count <> 0
        OR NOT pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CONNECT')
        OR NOT pg_catalog.has_schema_privilege(invoker_oid, 'public', 'USAGE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CREATE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'TEMPORARY')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_database AS foreign_database
            WHERE foreign_database.oid <> database_oid
                AND foreign_database.datallowconn
                AND (
                    pg_catalog.has_database_privilege(
                        invoker_oid,
                        foreign_database.oid,
                        'CONNECT'
                    )
                    OR pg_catalog.has_database_privilege(
                        invoker_oid,
                        foreign_database.oid,
                        'CREATE'
                    )
                    OR pg_catalog.has_database_privilege(
                        invoker_oid,
                        foreign_database.oid,
                        'TEMPORARY'
                    )
                )
        )
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
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_capability_drift';
    END IF;

    WITH violations(kind) AS (
        SELECT 'system_namespace'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'system_relation'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'system_attribute'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'system_sequence'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'system_function'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'system_type'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'application_relation'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'INSERT'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'UPDATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'DELETE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'TRUNCATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'REFERENCES'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'TRIGGER'
                    )
                )
        )
        UNION ALL
        SELECT 'application_attribute'
        WHERE EXISTS (
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
        UNION ALL
        SELECT 'application_sequence'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND sequence.relkind = 'S'
                AND (
                    pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'USAGE'
                    )
                    OR pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'UPDATE'
                    )
                )
        )
        UNION ALL
        SELECT 'parameter_acl'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege
            WHERE privilege.grantee IN (0, invoker_oid)
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
        )
        UNION ALL
        SELECT 'large_object'
        WHERE EXISTS (
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
    )
    SELECT pg_catalog.count(*)
    INTO unsafe_system_count
    FROM violations;

    IF unsafe_system_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_system_capability_drift';
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
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_identity_drift';
    END IF;

    database_name := pg_catalog.current_database()::TEXT;
    executor_role := session_user::TEXT;
    checked_at := pg_catalog.clock_timestamp();
    RETURN NEXT;
END;
$function$;

DO $readiness_body_postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
    invalid_function_count BIGINT;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    function_identity :=
        'public.starring_runtime_execution_database_readiness_v1()';

    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_owner_drift';
    END IF;

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
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_execution_database_function_grantee_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
            function_identity,
            grantee_name
        );
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_readiness_v1()'
        )
        AND (
            function_row.proowner <> common_owner
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
            OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                IS DISTINCT FROM ''
            OR pg_catalog.pg_get_function_result(function_row.oid)
                IS DISTINCT FROM 'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)'
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
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_readiness_v1()'
        ) IS NULL
        OR invalid_function_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_database_readiness_postflight_drift';
    END IF;
END;
$readiness_body_postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
