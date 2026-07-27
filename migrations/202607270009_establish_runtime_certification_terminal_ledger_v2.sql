SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_gateway_owners,
    public.runtime_deployments,
    public.runtime_serving_leases,
    public.runtime_slot_writer_fences_v2,
    public.runtime_certification_operations_v2,
    public.runtime_suspend_attempt_operations_v2,
    public.runtime_suspended_attempts_v2,
    public.runtime_suspend_attempt_completions_v2,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_startup_recovery_actions_v2,
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
    private_schema_owner OID;
    executor_role OID;
    executor_role_is_quarantined BOOLEAN;
    executor_membership_count BIGINT;
    other_client_session_count BIGINT;
    prepared_transaction_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    observation_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT namespace.nspowner
    INTO private_schema_owner
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname = 'starring_runtime_private_v2';

    SELECT privilege.grantee
    INTO executor_role
    FROM pg_catalog.pg_proc AS capability
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        capability.proacl,
        pg_catalog.acldefault('f', capability.proowner)
    )) AS privilege
    WHERE capability.oid = pg_catalog.to_regprocedure(
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

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO observation_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR private_schema_owner IS DISTINCT FROM common_owner
        OR NOT executor_role_is_quarantined
        OR executor_membership_count <> 0
        OR other_client_session_count <> 0
        OR prepared_transaction_count <> 0
        OR pg_catalog.to_regclass(
            'public.runtime_certification_operation_terminals_v2'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'public.reject_runtime_certification_terminal_mutation_v2()'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(smallint,text,text,text,text,text,bigint,bigint,text,text,bigint,bigint,timestamp with time zone,bytea)'
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
            '00824784a0b0276e2ef83b4e4094c274cffb50b9c640af61350a152dc112c835'
        OR readiness_digest IS DISTINCT FROM
            'c2cba3c5591876238f0ae0248b2c7c205953b6cde2a62705038a42fa9aa2aa81'
        OR observation_digest IS DISTINCT FROM
            '1bafd85ec4d2291c6ab7cf213acaed35fe637409a1ed8679881ee8686956df09'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_ledger_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(
    requested_record_format_version SMALLINT,
    requested_operation_id TEXT,
    requested_intent_fingerprint TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_deployment_revision BIGINT,
    requested_convergence_attempt_no BIGINT,
    requested_terminal_outcome_name TEXT,
    requested_resulting_phase TEXT,
    requested_resulting_deployment_revision BIGINT,
    requested_resulting_convergence_attempt_no BIGINT,
    requested_terminal_at TIMESTAMPTZ,
    terminal_receipt_bytes BYTEA
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    operation_id_bytes BYTEA;
    intent_fingerprint_bytes BYTEA;
    tenant_id_bytes BYTEA;
    installation_id_bytes BYTEA;
    deployment_id_bytes BYTEA;
    terminal_outcome_name_bytes BYTEA;
    resulting_phase_bytes BYTEA;
    canonical_payload BYTEA;
BEGIN
    operation_id_bytes := pg_catalog.convert_to(
        requested_operation_id,
        'UTF8'
    );
    intent_fingerprint_bytes := pg_catalog.convert_to(
        requested_intent_fingerprint,
        'UTF8'
    );
    tenant_id_bytes := pg_catalog.convert_to(requested_tenant_id, 'UTF8');
    installation_id_bytes := pg_catalog.convert_to(
        requested_installation_id,
        'UTF8'
    );
    deployment_id_bytes := pg_catalog.convert_to(
        requested_deployment_id,
        'UTF8'
    );
    terminal_outcome_name_bytes := pg_catalog.convert_to(
        requested_terminal_outcome_name,
        'UTF8'
    );
    resulting_phase_bytes := pg_catalog.convert_to(
        requested_resulting_phase,
        'UTF8'
    );
    canonical_payload :=
        pg_catalog.int2send(requested_record_format_version)
        || pg_catalog.int8send(
            pg_catalog.octet_length(operation_id_bytes)::BIGINT
        )
        || operation_id_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(intent_fingerprint_bytes)::BIGINT
        )
        || intent_fingerprint_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(tenant_id_bytes)::BIGINT
        )
        || tenant_id_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(installation_id_bytes)::BIGINT
        )
        || installation_id_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(deployment_id_bytes)::BIGINT
        )
        || deployment_id_bytes
        || pg_catalog.int8send(requested_deployment_revision)
        || pg_catalog.int8send(requested_convergence_attempt_no)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                terminal_outcome_name_bytes
            )::BIGINT
        )
        || terminal_outcome_name_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(resulting_phase_bytes)::BIGINT
        )
        || resulting_phase_bytes
        || pg_catalog.int8send(requested_resulting_deployment_revision)
        || pg_catalog.int8send(
            requested_resulting_convergence_attempt_no
        )
        || pg_catalog.timestamptz_send(requested_terminal_at)
        || pg_catalog.int8send(
            pg_catalog.octet_length(terminal_receipt_bytes)::BIGINT
        )
        || terminal_receipt_bytes;

    RETURN starring_runtime_private_v2.starring_runtime_framed_digest_v2(
        pg_catalog.convert_to(
            'starring.runtime.certification.terminal.v2',
            'UTF8'
        ) || pg_catalog.decode('00', 'hex'),
        canonical_payload
    );
END;
$function$;

CREATE TABLE public.runtime_certification_operation_terminals_v2 (
    record_format_version SMALLINT NOT NULL,
    operation_id TEXT PRIMARY KEY,
    intent_fingerprint TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    convergence_attempt_no BIGINT NOT NULL,
    terminal_outcome_name TEXT NOT NULL,
    resulting_phase TEXT NOT NULL,
    resulting_deployment_revision BIGINT NOT NULL,
    resulting_convergence_attempt_no BIGINT NOT NULL,
    terminal_at TIMESTAMPTZ NOT NULL,
    terminal_receipt_bytes BYTEA NOT NULL,
    terminal_receipt_digest TEXT NOT NULL,
    CONSTRAINT runtime_certification_operation_terminals_v2_format_check
        CHECK (record_format_version = 2),
    CONSTRAINT runtime_certification_operation_terminals_v2_outcome_check
        CHECK (
            (
                terminal_outcome_name = 'awaiting_reset'
                AND resulting_phase = 'reconciling_panels'
            )
            OR (
                terminal_outcome_name = 'certification_committed'
                AND resulting_phase = 'live'
            )
        ),
    CONSTRAINT runtime_certification_operation_terminals_v2_revision_check
        CHECK (
            deployment_revision < 9223372036854775807
            AND resulting_deployment_revision =
                deployment_revision + 1
            AND resulting_convergence_attempt_no =
                convergence_attempt_no
        ),
    CONSTRAINT runtime_certification_operation_terminals_v2_time_check
        CHECK (pg_catalog.isfinite(terminal_at)),
    CONSTRAINT runtime_certification_operation_terminals_v2_receipt_check
        CHECK (
            pg_catalog.octet_length(terminal_receipt_bytes)
                BETWEEN 1 AND 1048576
            AND terminal_receipt_digest ~ '^[0-9a-f]{64}$'
            AND terminal_receipt_digest <> pg_catalog.repeat('0', 64)
            AND terminal_receipt_digest =
                starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(
                    record_format_version,
                    operation_id,
                    intent_fingerprint,
                    tenant_id,
                    installation_id,
                    deployment_id,
                    deployment_revision,
                    convergence_attempt_no,
                    terminal_outcome_name,
                    resulting_phase,
                    resulting_deployment_revision,
                    resulting_convergence_attempt_no,
                    terminal_at,
                    terminal_receipt_bytes
                )
        )
);

CREATE FUNCTION public.reject_runtime_certification_terminal_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    gate_valid BOOLEAN;
    root_count BIGINT;
    setting_name TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_certification_terminal_action_v2',
            'starring.runtime_certification_terminal_operation_id_v2',
            'starring.runtime_certification_terminal_outcome_v2',
            'starring.runtime_certification_terminal_result_phase_v2',
            'starring.runtime_certification_terminal_result_revision_v2',
            'starring.runtime_certification_terminal_result_attempt_v2',
            'starring.runtime_certification_terminal_digest_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_certification_terminal_mutation_rejected';
    END IF;

    gate_valid := COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_action_v2',
                TRUE
            ), '') = 'insert'
        AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_operation_id_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM NEW.operation_id
        AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_outcome_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM NEW.terminal_outcome_name
        AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_result_phase_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM NEW.resulting_phase
        AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_result_revision_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM
                NEW.resulting_deployment_revision::TEXT
        AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_result_attempt_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM
                NEW.resulting_convergence_attempt_no::TEXT
        AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_certification_terminal_digest_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM NEW.terminal_receipt_digest;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_certification_terminal_action_v2',
        'starring.runtime_certification_terminal_operation_id_v2',
        'starring.runtime_certification_terminal_outcome_v2',
        'starring.runtime_certification_terminal_result_phase_v2',
        'starring.runtime_certification_terminal_result_revision_v2',
        'starring.runtime_certification_terminal_result_attempt_v2',
        'starring.runtime_certification_terminal_digest_v2'
    ]
    LOOP
        PERFORM pg_catalog.set_config(setting_name, '', TRUE);
    END LOOP;

    IF NOT gate_valid THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_certification_terminal_mutation_rejected';
    END IF;

    SELECT pg_catalog.count(*)
    INTO root_count
    FROM public.runtime_certification_operations_v2 AS operation
    WHERE operation.operation_id = NEW.operation_id
        AND operation.intent_fingerprint = NEW.intent_fingerprint
        AND operation.tenant_id = NEW.tenant_id
        AND operation.installation_id = NEW.installation_id
        AND operation.deployment_id = NEW.deployment_id
        AND operation.deployment_revision = NEW.deployment_revision
        AND operation.convergence_attempt_no = NEW.convergence_attempt_no;

    IF root_count <> 1 THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_certification_terminal_root_invalid';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_certification_terminals_v2_reject_row
BEFORE INSERT OR UPDATE OR DELETE
ON public.runtime_certification_operation_terminals_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_certification_terminal_mutation_v2();

CREATE TRIGGER runtime_certification_terminals_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_certification_operation_terminals_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_certification_terminal_mutation_v2();

REVOKE ALL ON TABLE
    public.runtime_certification_operation_terminals_v2
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_certification_terminal_mutation_v2()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(
        SMALLINT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        BIGINT,
        TIMESTAMPTZ,
        BYTEA
    )
FROM PUBLIC;

DO $patch_observation$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    previous_fragment :=
        '    reservation_count BIGINT;' || E'\n' ||
        '    exact_awaiting_reservation_count BIGINT;' || E'\n' ||
        '    invalid_suspend_attempt_count BIGINT;';
    next_fragment :=
        '    reservation_count BIGINT;' || E'\n' ||
        '    unresolved_reservation_count BIGINT;' || E'\n' ||
        '    exact_terminal_reservation_count BIGINT;' || E'\n' ||
        '    invalid_reservation_count BIGINT;' || E'\n' ||
        '    invalid_suspend_attempt_count BIGINT;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_observation_declaration_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        public.runtime_certification_operations_v2,' || E'\n' ||
        '        public.runtime_suspend_attempt_operations_v2,';
    next_fragment :=
        '        public.runtime_certification_operations_v2,' || E'\n' ||
        '        public.runtime_certification_operation_terminals_v2,' || E'\n' ||
        '        public.runtime_suspend_attempt_operations_v2,';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_observation_lock_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    SELECT' || E'\n' ||
        '        pg_catalog.count(*),' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE deployment.phase = ''awaiting_gateway_ready''' || E'\n' ||
        '                AND deployment.revision =' || E'\n' ||
        '                    reservation.deployment_revision' || E'\n' ||
        '                AND deployment.convergence_attempt_no =' || E'\n' ||
        '                    reservation.convergence_attempt_no' || E'\n' ||
        '                AND deployment.snapshot #>> ''{phase,phase}'' =' || E'\n' ||
        '                    ''awaiting_gateway_ready''' || E'\n' ||
        '                AND deployment.snapshot ->> ''revision'' =' || E'\n' ||
        '                    reservation.deployment_revision::TEXT' || E'\n' ||
        '                AND deployment.controller_id IS NOT NULL' || E'\n' ||
        '                AND deployment.controller_fencing_token IS NOT NULL' || E'\n' ||
        '                AND deployment.last_controller_id =' || E'\n' ||
        '                    deployment.controller_id' || E'\n' ||
        '                AND deployment.last_fencing_token =' || E'\n' ||
        '                    deployment.controller_fencing_token' || E'\n' ||
        '        )' || E'\n' ||
        '    INTO reservation_count, exact_awaiting_reservation_count' || E'\n' ||
        '    FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '    LEFT JOIN public.runtime_deployments AS deployment' || E'\n' ||
        '        ON deployment.tenant_id = reservation.tenant_id' || E'\n' ||
        '        AND deployment.installation_id =' || E'\n' ||
        '            reservation.installation_id' || E'\n' ||
        '        AND deployment.deployment_id = reservation.deployment_id;' || E'\n' ||
        E'\n' ||
        '    IF reservation_count <> exact_awaiting_reservation_count THEN' || E'\n' ||
        '        outcome_name := ''ambiguous'';' || E'\n' ||
        '        serving_state_name := ''ambiguous'';' || E'\n' ||
        '        serving_count := NULL;' || E'\n' ||
        '        serving_earliest_expiry := NULL;' || E'\n' ||
        '        serving_retry_after_milliseconds := NULL;' || E'\n' ||
        '        RETURN NEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    next_fragment :=
        '    SELECT' || E'\n' ||
        '        pg_catalog.count(*),' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE terminal.operation_id IS NULL' || E'\n' ||
        '                AND deployment.phase = ''awaiting_gateway_ready''' || E'\n' ||
        '                AND deployment.revision =' || E'\n' ||
        '                    reservation.deployment_revision' || E'\n' ||
        '                AND deployment.convergence_attempt_no =' || E'\n' ||
        '                    reservation.convergence_attempt_no' || E'\n' ||
        '                AND deployment.snapshot #>> ''{phase,phase}'' =' || E'\n' ||
        '                    ''awaiting_gateway_ready''' || E'\n' ||
        '                AND deployment.snapshot ->> ''revision'' =' || E'\n' ||
        '                    reservation.deployment_revision::TEXT' || E'\n' ||
        '                AND deployment.controller_id IS NOT NULL' || E'\n' ||
        '                AND deployment.controller_fencing_token IS NOT NULL' || E'\n' ||
        '                AND deployment.last_controller_id =' || E'\n' ||
        '                    deployment.controller_id' || E'\n' ||
        '                AND deployment.last_fencing_token =' || E'\n' ||
        '                    deployment.controller_fencing_token' || E'\n' ||
        '        ),' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE terminal.operation_id IS NOT NULL' || E'\n' ||
        '                AND deployment.snapshot ->> ''revision'' =' || E'\n' ||
        '                    deployment.revision::TEXT' || E'\n' ||
        '                AND deployment.snapshot #>> ''{phase,phase}'' =' || E'\n' ||
        '                    deployment.phase' || E'\n' ||
        '                AND (' || E'\n' ||
        '                    (' || E'\n' ||
        '                        deployment.revision =' || E'\n' ||
        '                            terminal.resulting_deployment_revision' || E'\n' ||
        '                        AND deployment.phase =' || E'\n' ||
        '                            terminal.resulting_phase' || E'\n' ||
        '                        AND deployment.convergence_attempt_no =' || E'\n' ||
        '                            terminal.resulting_convergence_attempt_no' || E'\n' ||
        '                    )' || E'\n' ||
        '                    OR (' || E'\n' ||
        '                        deployment.revision >' || E'\n' ||
        '                            terminal.resulting_deployment_revision' || E'\n' ||
        '                        AND deployment.convergence_attempt_no >=' || E'\n' ||
        '                            terminal.resulting_convergence_attempt_no' || E'\n' ||
        '                    )' || E'\n' ||
        '                )' || E'\n' ||
        '        )' || E'\n' ||
        '    INTO' || E'\n' ||
        '        reservation_count,' || E'\n' ||
        '        unresolved_reservation_count,' || E'\n' ||
        '        exact_terminal_reservation_count' || E'\n' ||
        '    FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '    LEFT JOIN public.runtime_certification_operation_terminals_v2' || E'\n' ||
        '        AS terminal' || E'\n' ||
        '        ON terminal.operation_id = reservation.operation_id' || E'\n' ||
        '        AND terminal.intent_fingerprint =' || E'\n' ||
        '            reservation.intent_fingerprint' || E'\n' ||
        '        AND terminal.tenant_id = reservation.tenant_id' || E'\n' ||
        '        AND terminal.installation_id =' || E'\n' ||
        '            reservation.installation_id' || E'\n' ||
        '        AND terminal.deployment_id = reservation.deployment_id' || E'\n' ||
        '        AND terminal.deployment_revision =' || E'\n' ||
        '            reservation.deployment_revision' || E'\n' ||
        '        AND terminal.convergence_attempt_no =' || E'\n' ||
        '            reservation.convergence_attempt_no' || E'\n' ||
        '    LEFT JOIN public.runtime_deployments AS deployment' || E'\n' ||
        '        ON deployment.tenant_id = reservation.tenant_id' || E'\n' ||
        '        AND deployment.installation_id =' || E'\n' ||
        '            reservation.installation_id' || E'\n' ||
        '        AND deployment.deployment_id = reservation.deployment_id;' || E'\n' ||
        E'\n' ||
        '    invalid_reservation_count := reservation_count' || E'\n' ||
        '        - unresolved_reservation_count' || E'\n' ||
        '        - exact_terminal_reservation_count;' || E'\n' ||
        '    IF invalid_reservation_count <> 0 THEN' || E'\n' ||
        '        outcome_name := ''ambiguous'';' || E'\n' ||
        '        serving_state_name := ''ambiguous'';' || E'\n' ||
        '        serving_count := NULL;' || E'\n' ||
        '        serving_earliest_expiry := NULL;' || E'\n' ||
        '        serving_retry_after_milliseconds := NULL;' || E'\n' ||
        '        RETURN NEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_observation_classifier_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF reservation_count > 4294967295';
    next_fragment :=
        '    IF unresolved_reservation_count > 4294967295';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_observation_bound_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    recoverable_awaiting_certification_count := reservation_count;';
    next_fragment :=
        '    recoverable_awaiting_certification_count :=' || E'\n' ||
        '        unresolved_reservation_count;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_observation_projection_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_observation$;

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
        '            (pg_catalog.to_regclass(''public.runtime_certification_operation_terminals_v2'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)''' || E'\n' ||
        '        )';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(smallint,text,text,text,text,text,bigint,bigint,text,text,bigint,bigint,timestamp with time zone,bytea)''' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 769' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''659a6609f468edabc6135b5b056d58ac1929ea223471155e05325ea0d6da5a87'';';
    next_fragment :=
        'RETURN observed_count = 796' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''51f3694196e13c3b5bd21421ccdaa595291f2832063802df4967f502606bf0b5'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_manifest_expectation_patch_drift';
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
        '            (''public.runtime_certification_operation_terminals_v2''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''public.reject_runtime_suspend_attempt_ledger_mutation_v2()''),';
    next_fragment :=
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''public.reject_runtime_certification_terminal_mutation_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(smallint,text,text,text,text,text,bigint,bigint,text,text,bigint,bigint,timestamp with time zone,bytea)''),' || E'\n' ||
        '            (''public.reject_runtime_suspend_attempt_ledger_mutation_v2()''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''00824784a0b0276e2ef83b4e4094c274cffb50b9c640af61350a152dc112c835''::TEXT';
    next_fragment :=
        '''1fa238c260d3bdfa7b0c914a42616c2889c02d92253829c8bafa63a8a255a3f7''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_readiness_manifest_digest_patch_drift';
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
    observation_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT privilege.grantee
    INTO executor_role
    FROM pg_catalog.pg_proc AS capability
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        capability.proacl,
        pg_catalog.acldefault('f', capability.proowner)
    )) AS privilege
    WHERE capability.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
    ORDER BY privilege.grantee
    LIMIT 1;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_certification_operation_terminals_v2')
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
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.reject_runtime_certification_terminal_mutation_v2()',
                'v'::"char",
                'u'::"char",
                TRUE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(smallint,text,text,text,text,text,bigint,bigint,text,text,bigint,bigint,timestamp with time zone,bytea)',
                'i'::"char",
                's'::"char",
                FALSE
            )
    ) AS expected(identity, volatility, parallel_kind, security_definer)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR function_row.proisstrict <>
            (expected.identity LIKE
                'starring_runtime_private_v2.%')
        OR function_row.proparallel <> expected.parallel_kind
        OR function_row.prosecdef <> expected.security_definer
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
            (
                'runtime_certification_terminals_v2_reject_row',
                31
            ),
            (
                'runtime_certification_terminals_v2_reject_truncate',
                34
            )
    ) AS expected(trigger_name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            'public.runtime_certification_operation_terminals_v2'
        )
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgtype <> expected.trigger_type;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(capability.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO observation_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(capability.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(capability.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_trigger_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_certification_operation_terminals_v2
        ) <> 0
        OR observation_digest IS DISTINCT FROM
            '7153d2dcf3eaa6a6534368eead9f40c157c63372c879ce99adf173eb3d23f306'
        OR manifest_digest IS DISTINCT FROM
            '1fa238c260d3bdfa7b0c914a42616c2889c02d92253829c8bafa63a8a255a3f7'
        OR readiness_digest IS DISTINCT FROM
            'a5191ef59e5365476860af1150a176049ef00c5b0d6c3f7cfe40e0b5be9d738a'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_terminal_ledger_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
