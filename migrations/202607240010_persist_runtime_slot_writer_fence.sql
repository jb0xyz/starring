SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.automation_installations,
    public.runtime_deployments,
    public.runtime_serving_leases,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
BEGIN
    IF pg_catalog.to_regclass(
            'public.runtime_slot_writer_fences_v2'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'public.reject_runtime_slot_writer_fence_mutation_v2()'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(text,text)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()'
        ) IS NOT NULL
        OR EXISTS (
            SELECT 1
            FROM public.runtime_product_operations_v2
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2
        )
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(
                        pg_catalog.to_regprocedure(
                            'public.starring_runtime_exact_target_schema_manifest_v1()'
                        )
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) <> '5b705cf2cd0fd7562d04663a6984259b33d36ee66cd5689159f11c44d0632b83'
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(
                        pg_catalog.to_regprocedure(
                            'public.starring_runtime_exact_target_database_readiness_v1()'
                        )
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) <> '4e8a991373f8fdd0d619307e811200d2551e82feba19e7c1d252115001a02123'
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(
                        pg_catalog.to_regprocedure(
                            'public.starring_runtime_serving_schema_manifest_v1()'
                        )
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) <> '133f73f8eb70606e023af29294ade8bb593b2adc06db3e663bdd42d7693a43be'
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(
                        pg_catalog.to_regprocedure(
                            'public.starring_runtime_serving_database_readiness_v1()'
                        )
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) <> '45791dded732504e4f235f17153646affa14f6e94e6b4fdc0874f2279e1533a7'
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(
                        pg_catalog.to_regprocedure(
                            'public.starring_runtime_execution_schema_manifest_v1()'
                        )
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) <> '331a95180a75109385566b0b1b0659e247e5619cf02e2f61ee89904a2751856b'
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(
                        pg_catalog.to_regprocedure(
                            'public.starring_runtime_execution_database_readiness_v1()'
                        )
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) <> '3e2d46d692daf8bd9cff68f00459f00f6b8bf314378a663727b94493d7e45279'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_preflight_drift';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_drain_intents_v2
ADD CONSTRAINT runtime_drain_intents_v2_fence_identity_unique UNIQUE (
    drain_intent_id,
    product_operation_id,
    tenant_id,
    installation_id,
    deployment_id,
    slot_guild_id,
    slot_ruleset_key,
    expected_revision
);

CREATE UNIQUE INDEX runtime_drain_intents_v2_one_pending_per_slot
ON public.runtime_drain_intents_v2 (
    slot_guild_id,
    slot_ruleset_key
)
WHERE intent_state = 'pending';

CREATE TABLE public.runtime_slot_writer_fences_v2 (
    slot_guild_id TEXT NOT NULL,
    slot_ruleset_key TEXT NOT NULL,
    writer_epoch BIGINT NOT NULL,
    pending_drain_intent_id TEXT,
    pending_product_operation_id TEXT,
    pending_tenant_id TEXT,
    pending_installation_id TEXT,
    pending_deployment_id TEXT,
    pending_expected_revision BIGINT,
    pending_marked_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_slot_writer_fences_v2_pkey PRIMARY KEY (
        slot_guild_id,
        slot_ruleset_key
    ),
    CONSTRAINT runtime_slot_writer_fences_v2_installation_fk FOREIGN KEY (
        slot_guild_id,
        slot_ruleset_key
    ) REFERENCES public.automation_installations (
        discord_guild_id,
        ruleset_key
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_slot_writer_fences_v2_pending_fk FOREIGN KEY (
        pending_drain_intent_id,
        pending_product_operation_id,
        pending_tenant_id,
        pending_installation_id,
        pending_deployment_id,
        slot_guild_id,
        slot_ruleset_key,
        pending_expected_revision
    ) REFERENCES public.runtime_drain_intents_v2 (
        drain_intent_id,
        product_operation_id,
        tenant_id,
        installation_id,
        deployment_id,
        slot_guild_id,
        slot_ruleset_key,
        expected_revision
    ) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT runtime_slot_writer_fences_v2_pending_intent_unique UNIQUE (
        pending_drain_intent_id
    ),
    CONSTRAINT runtime_slot_writer_fences_v2_pending_product_unique UNIQUE (
        pending_product_operation_id
    ),
    CONSTRAINT runtime_slot_writer_fences_v2_slot_check CHECK (
        slot_guild_id ~ '^[1-9][0-9]{0,19}$'
        AND (
            pg_catalog.length(slot_guild_id) < 20
            OR slot_guild_id COLLATE pg_catalog."C"
                <= '18446744073709551615' COLLATE pg_catalog."C"
        )
        AND slot_ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT runtime_slot_writer_fences_v2_epoch_check CHECK (
        writer_epoch BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_slot_writer_fences_v2_pending_shape_check CHECK (
        (
            pending_drain_intent_id IS NULL
            AND pending_product_operation_id IS NULL
            AND pending_tenant_id IS NULL
            AND pending_installation_id IS NULL
            AND pending_deployment_id IS NULL
            AND pending_expected_revision IS NULL
            AND pending_marked_at IS NULL
        ) OR (
            pending_drain_intent_id IS NOT NULL
            AND pending_product_operation_id IS NOT NULL
            AND pending_tenant_id IS NOT NULL
            AND pending_installation_id IS NOT NULL
            AND pending_deployment_id IS NOT NULL
            AND pending_expected_revision IS NOT NULL
            AND pending_marked_at IS NOT NULL
            AND pending_drain_intent_id ~ '^[0-9a-f]{32}$'
            AND pending_product_operation_id ~ '^[0-9a-f]{32}$'
            AND pending_tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND pending_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND pending_deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND pending_expected_revision
                BETWEEN 1 AND 9223372036854775807
            AND pg_catalog.isfinite(pending_marked_at)
        )
    ),
    CONSTRAINT runtime_slot_writer_fences_v2_updated_at_check CHECK (
        pg_catalog.isfinite(updated_at)
    )
);

INSERT INTO public.runtime_slot_writer_fences_v2 (
    slot_guild_id,
    slot_ruleset_key,
    writer_epoch,
    pending_drain_intent_id,
    pending_product_operation_id,
    pending_tenant_id,
    pending_installation_id,
    pending_deployment_id,
    pending_expected_revision,
    pending_marked_at,
    updated_at
)
SELECT
    installation.discord_guild_id,
    installation.ruleset_key,
    1,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    pg_catalog.clock_timestamp()
FROM public.automation_installations AS installation
ORDER BY installation.discord_guild_id, installation.ruleset_key;

CREATE FUNCTION public.reject_runtime_slot_writer_fence_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    gate_action TEXT;
    gate_slot_guild_id TEXT;
    gate_slot_ruleset_key TEXT;
    gate_expected_epoch TEXT;
    gate_drain_intent_id TEXT;
    gate_product_operation_id TEXT;
    gate_tenant_id TEXT;
    gate_installation_id TEXT;
    gate_deployment_id TEXT;
    gate_expected_revision TEXT;
    gate_marked_at TEXT;
    setting_name TEXT;
BEGIN
    gate_action := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_action_v2',
        TRUE
    );
    gate_slot_guild_id := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        TRUE
    );
    gate_slot_ruleset_key := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        TRUE
    );
    gate_expected_epoch := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        TRUE
    );
    gate_drain_intent_id := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_drain_intent_id_v2',
        TRUE
    );
    gate_product_operation_id := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_product_operation_id_v2',
        TRUE
    );
    gate_tenant_id := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_tenant_id_v2',
        TRUE
    );
    gate_installation_id := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_installation_id_v2',
        TRUE
    );
    gate_deployment_id := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_deployment_id_v2',
        TRUE
    );
    gate_expected_revision := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_expected_revision_v2',
        TRUE
    );
    gate_marked_at := pg_catalog.current_setting(
        'starring.runtime_slot_writer_fence_marked_at_v2',
        TRUE
    );

    IF TG_OP = 'INSERT'
        AND gate_action = 'create'
        AND gate_slot_guild_id = NEW.slot_guild_id
        AND gate_slot_ruleset_key = NEW.slot_ruleset_key
        AND COALESCE(gate_expected_epoch, '') = ''
        AND COALESCE(gate_drain_intent_id, '') = ''
        AND COALESCE(gate_product_operation_id, '') = ''
        AND COALESCE(gate_tenant_id, '') = ''
        AND COALESCE(gate_installation_id, '') = ''
        AND COALESCE(gate_deployment_id, '') = ''
        AND COALESCE(gate_expected_revision, '') = ''
        AND COALESCE(gate_marked_at, '') = ''
        AND NEW.writer_epoch = 1
        AND NEW.pending_drain_intent_id IS NULL
        AND NEW.pending_product_operation_id IS NULL
        AND NEW.pending_tenant_id IS NULL
        AND NEW.pending_installation_id IS NULL
        AND NEW.pending_deployment_id IS NULL
        AND NEW.pending_expected_revision IS NULL
        AND NEW.pending_marked_at IS NULL
    THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_slot_writer_fence_action_v2',
            'starring.runtime_slot_writer_fence_slot_guild_id_v2',
            'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
            'starring.runtime_slot_writer_fence_expected_epoch_v2',
            'starring.runtime_slot_writer_fence_drain_intent_id_v2',
            'starring.runtime_slot_writer_fence_product_operation_id_v2',
            'starring.runtime_slot_writer_fence_tenant_id_v2',
            'starring.runtime_slot_writer_fence_installation_id_v2',
            'starring.runtime_slot_writer_fence_deployment_id_v2',
            'starring.runtime_slot_writer_fence_expected_revision_v2',
            'starring.runtime_slot_writer_fence_marked_at_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;
        RETURN NEW;
    END IF;

    IF TG_OP = 'UPDATE'
        AND gate_action = 'advance'
        AND gate_slot_guild_id = OLD.slot_guild_id
        AND gate_slot_ruleset_key = OLD.slot_ruleset_key
        AND gate_expected_epoch = OLD.writer_epoch::TEXT
        AND COALESCE(gate_drain_intent_id, '') = ''
        AND COALESCE(gate_product_operation_id, '') = ''
        AND COALESCE(gate_tenant_id, '') = ''
        AND COALESCE(gate_installation_id, '') = ''
        AND COALESCE(gate_deployment_id, '') = ''
        AND COALESCE(gate_expected_revision, '') = ''
        AND COALESCE(gate_marked_at, '') = ''
        AND NEW.slot_guild_id = OLD.slot_guild_id
        AND NEW.slot_ruleset_key = OLD.slot_ruleset_key
        AND NEW.writer_epoch = OLD.writer_epoch + 1
        AND OLD.pending_drain_intent_id IS NULL
        AND NEW.pending_drain_intent_id IS NULL
        AND NEW.pending_product_operation_id IS NULL
        AND NEW.pending_tenant_id IS NULL
        AND NEW.pending_installation_id IS NULL
        AND NEW.pending_deployment_id IS NULL
        AND NEW.pending_expected_revision IS NULL
        AND NEW.pending_marked_at IS NULL
        AND NEW.updated_at >= OLD.updated_at
    THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_slot_writer_fence_action_v2',
            'starring.runtime_slot_writer_fence_slot_guild_id_v2',
            'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
            'starring.runtime_slot_writer_fence_expected_epoch_v2',
            'starring.runtime_slot_writer_fence_drain_intent_id_v2',
            'starring.runtime_slot_writer_fence_product_operation_id_v2',
            'starring.runtime_slot_writer_fence_tenant_id_v2',
            'starring.runtime_slot_writer_fence_installation_id_v2',
            'starring.runtime_slot_writer_fence_deployment_id_v2',
            'starring.runtime_slot_writer_fence_expected_revision_v2',
            'starring.runtime_slot_writer_fence_marked_at_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;
        RETURN NEW;
    END IF;

    IF TG_OP = 'UPDATE'
        AND gate_action = 'mark_drain'
        AND gate_slot_guild_id = OLD.slot_guild_id
        AND gate_slot_ruleset_key = OLD.slot_ruleset_key
        AND gate_expected_epoch = OLD.writer_epoch::TEXT
        AND gate_drain_intent_id = NEW.pending_drain_intent_id
        AND gate_product_operation_id = NEW.pending_product_operation_id
        AND gate_tenant_id = NEW.pending_tenant_id
        AND gate_installation_id = NEW.pending_installation_id
        AND gate_deployment_id = NEW.pending_deployment_id
        AND gate_expected_revision = NEW.pending_expected_revision::TEXT
        AND gate_marked_at = NEW.pending_marked_at::TEXT
        AND NEW.slot_guild_id = OLD.slot_guild_id
        AND NEW.slot_ruleset_key = OLD.slot_ruleset_key
        AND NEW.writer_epoch = OLD.writer_epoch + 1
        AND OLD.pending_drain_intent_id IS NULL
        AND OLD.pending_product_operation_id IS NULL
        AND OLD.pending_tenant_id IS NULL
        AND OLD.pending_installation_id IS NULL
        AND OLD.pending_deployment_id IS NULL
        AND OLD.pending_expected_revision IS NULL
        AND OLD.pending_marked_at IS NULL
        AND NEW.updated_at >= OLD.updated_at
    THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_slot_writer_fence_action_v2',
            'starring.runtime_slot_writer_fence_slot_guild_id_v2',
            'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
            'starring.runtime_slot_writer_fence_expected_epoch_v2',
            'starring.runtime_slot_writer_fence_drain_intent_id_v2',
            'starring.runtime_slot_writer_fence_product_operation_id_v2',
            'starring.runtime_slot_writer_fence_tenant_id_v2',
            'starring.runtime_slot_writer_fence_installation_id_v2',
            'starring.runtime_slot_writer_fence_deployment_id_v2',
            'starring.runtime_slot_writer_fence_expected_revision_v2',
            'starring.runtime_slot_writer_fence_marked_at_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;
        RETURN NEW;
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_slot_writer_fence_mutation_rejected';
END;
$function$;

CREATE TRIGGER runtime_slot_writer_fences_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_slot_writer_fences_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_slot_writer_fence_mutation_v2();

CREATE TRIGGER runtime_slot_writer_fences_v2_reject_truncate
BEFORE TRUNCATE ON public.runtime_slot_writer_fences_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_slot_writer_fence_mutation_v2();

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    setting_name TEXT;
    next_epoch BIGINT;
BEGIN
    IF requested_slot_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.length(requested_slot_guild_id) = 20
            AND requested_slot_guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR requested_slot_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                'starring.runtime_slot_writer_fence_action_v2',
                'starring.runtime_slot_writer_fence_slot_guild_id_v2',
                'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
                'starring.runtime_slot_writer_fence_expected_epoch_v2',
                'starring.runtime_slot_writer_fence_drain_intent_id_v2',
                'starring.runtime_slot_writer_fence_product_operation_id_v2',
                'starring.runtime_slot_writer_fence_tenant_id_v2',
                'starring.runtime_slot_writer_fence_installation_id_v2',
                'starring.runtime_slot_writer_fence_deployment_id_v2',
                'starring.runtime_slot_writer_fence_expected_revision_v2',
                'starring.runtime_slot_writer_fence_marked_at_v2'
            ]) AS setting(setting_name)
            WHERE COALESCE(pg_catalog.current_setting(
                setting.setting_name,
                TRUE
            ), '') <> ''
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.automation_installations AS installation
            WHERE installation.discord_guild_id = requested_slot_guild_id
                AND installation.ruleset_key = requested_slot_ruleset_key
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_slot_writer_fence_create_invalid';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_action_v2',
        'create',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        requested_slot_guild_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        requested_slot_ruleset_key,
        TRUE
    );

    INSERT INTO public.runtime_slot_writer_fences_v2 (
        slot_guild_id,
        slot_ruleset_key,
        writer_epoch,
        pending_drain_intent_id,
        pending_product_operation_id,
        pending_tenant_id,
        pending_installation_id,
        pending_deployment_id,
        pending_expected_revision,
        pending_marked_at,
        updated_at
    ) VALUES (
        requested_slot_guild_id,
        requested_slot_ruleset_key,
        1,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        pg_catalog.clock_timestamp()
    )
    RETURNING writer_epoch INTO next_epoch;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_slot_writer_fence_action_v2',
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        'starring.runtime_slot_writer_fence_drain_intent_id_v2',
        'starring.runtime_slot_writer_fence_product_operation_id_v2',
        'starring.runtime_slot_writer_fence_tenant_id_v2',
        'starring.runtime_slot_writer_fence_installation_id_v2',
        'starring.runtime_slot_writer_fence_deployment_id_v2',
        'starring.runtime_slot_writer_fence_expected_revision_v2',
        'starring.runtime_slot_writer_fence_marked_at_v2'
    ]
    LOOP
        IF COALESCE(pg_catalog.current_setting(setting_name, TRUE), '') <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_slot_writer_fence_gate_consumption_invalid';
        END IF;
    END LOOP;

    RETURN next_epoch;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT
)
RETURNS TABLE(
    writer_epoch BIGINT,
    pending_drain_intent_id TEXT,
    pending_product_operation_id TEXT,
    pending_tenant_id TEXT,
    pending_installation_id TEXT,
    pending_deployment_id TEXT,
    pending_expected_revision BIGINT,
    pending_marked_at TIMESTAMPTZ,
    observed_at TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
BEGIN
    SELECT fence.*
    INTO fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = requested_slot_guild_id
        AND fence.slot_ruleset_key = requested_slot_ruleset_key
    FOR UPDATE;

    IF NOT FOUND
        OR fence_row.writer_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR (
            fence_row.pending_drain_intent_id IS NULL
            AND EXISTS (
                SELECT 1
                FROM public.runtime_drain_intents_v2 AS drain
                WHERE drain.slot_guild_id = fence_row.slot_guild_id
                    AND drain.slot_ruleset_key
                        = fence_row.slot_ruleset_key
                    AND drain.intent_state = 'pending'
            )
        )
        OR (
            fence_row.pending_drain_intent_id IS NOT NULL
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_drain_intents_v2 AS drain
                WHERE drain.drain_intent_id
                        = fence_row.pending_drain_intent_id
                    AND drain.product_operation_id
                        = fence_row.pending_product_operation_id
                    AND drain.tenant_id = fence_row.pending_tenant_id
                    AND drain.installation_id
                        = fence_row.pending_installation_id
                    AND drain.deployment_id
                        = fence_row.pending_deployment_id
                    AND drain.slot_guild_id = fence_row.slot_guild_id
                    AND drain.slot_ruleset_key = fence_row.slot_ruleset_key
                    AND drain.expected_revision
                        = fence_row.pending_expected_revision
                    AND drain.intent_state = 'pending'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_product_drain_state_invalid';
    END IF;

    RETURN QUERY SELECT
        fence_row.writer_epoch,
        fence_row.pending_drain_intent_id,
        fence_row.pending_product_operation_id,
        fence_row.pending_tenant_id,
        fence_row.pending_installation_id,
        fence_row.pending_deployment_id,
        fence_row.pending_expected_revision,
        fence_row.pending_marked_at,
        pg_catalog.clock_timestamp();
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT,
    requested_expected_epoch BIGINT
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    next_epoch BIGINT;
    setting_name TEXT;
BEGIN
    IF requested_expected_epoch NOT BETWEEN 1 AND 9223372036854775806
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                'starring.runtime_slot_writer_fence_action_v2',
                'starring.runtime_slot_writer_fence_slot_guild_id_v2',
                'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
                'starring.runtime_slot_writer_fence_expected_epoch_v2',
                'starring.runtime_slot_writer_fence_drain_intent_id_v2',
                'starring.runtime_slot_writer_fence_product_operation_id_v2',
                'starring.runtime_slot_writer_fence_tenant_id_v2',
                'starring.runtime_slot_writer_fence_installation_id_v2',
                'starring.runtime_slot_writer_fence_deployment_id_v2',
                'starring.runtime_slot_writer_fence_expected_revision_v2',
                'starring.runtime_slot_writer_fence_marked_at_v2'
            ]) AS setting(setting_name)
            WHERE COALESCE(pg_catalog.current_setting(
                setting.setting_name,
                TRUE
            ), '') <> ''
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_product_drain_state_invalid';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_action_v2',
        'advance',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        requested_slot_guild_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        requested_slot_ruleset_key,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        requested_expected_epoch::TEXT,
        TRUE
    );

    UPDATE public.runtime_slot_writer_fences_v2 AS fence
    SET writer_epoch = fence.writer_epoch + 1,
        updated_at = GREATEST(
            fence.updated_at,
            pg_catalog.clock_timestamp()
        )
    WHERE fence.slot_guild_id = requested_slot_guild_id
        AND fence.slot_ruleset_key = requested_slot_ruleset_key
        AND fence.writer_epoch = requested_expected_epoch
        AND fence.pending_drain_intent_id IS NULL
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = fence.slot_guild_id
                AND drain.slot_ruleset_key = fence.slot_ruleset_key
                AND drain.intent_state = 'pending'
        )
    RETURNING fence.writer_epoch INTO next_epoch;

    IF NOT FOUND THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_slot_writer_fence_action_v2',
            'starring.runtime_slot_writer_fence_slot_guild_id_v2',
            'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
            'starring.runtime_slot_writer_fence_expected_epoch_v2',
            'starring.runtime_slot_writer_fence_drain_intent_id_v2',
            'starring.runtime_slot_writer_fence_product_operation_id_v2',
            'starring.runtime_slot_writer_fence_tenant_id_v2',
            'starring.runtime_slot_writer_fence_installation_id_v2',
            'starring.runtime_slot_writer_fence_deployment_id_v2',
            'starring.runtime_slot_writer_fence_expected_revision_v2',
            'starring.runtime_slot_writer_fence_marked_at_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;

        SELECT fence.*
        INTO fence_row
        FROM public.runtime_slot_writer_fences_v2 AS fence
        WHERE fence.slot_guild_id = requested_slot_guild_id
            AND fence.slot_ruleset_key = requested_slot_ruleset_key;

        IF NOT FOUND
            OR fence_row.writer_epoch = 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_product_drain_state_invalid';
        ELSIF fence_row.pending_drain_intent_id IS NOT NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX007',
                MESSAGE = 'runtime_execution_product_drain_pending';
        ELSIF EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = requested_slot_guild_id
                AND drain.slot_ruleset_key = requested_slot_ruleset_key
                AND drain.intent_state = 'pending'
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_product_drain_state_invalid';
        END IF;

        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_stale';
    END IF;

    RETURN next_epoch;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT,
    requested_expected_epoch BIGINT,
    requested_drain_intent_id TEXT,
    requested_product_operation_id TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_expected_revision BIGINT
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    mark_clock TIMESTAMPTZ;
    next_epoch BIGINT;
    setting_name TEXT;
BEGIN
    IF requested_expected_epoch NOT BETWEEN 1 AND 9223372036854775806
        OR requested_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR requested_product_operation_id !~ '^[0-9a-f]{32}$'
        OR requested_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_expected_revision NOT BETWEEN 1 AND 9223372036854775807
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                'starring.runtime_slot_writer_fence_action_v2',
                'starring.runtime_slot_writer_fence_slot_guild_id_v2',
                'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
                'starring.runtime_slot_writer_fence_expected_epoch_v2',
                'starring.runtime_slot_writer_fence_drain_intent_id_v2',
                'starring.runtime_slot_writer_fence_product_operation_id_v2',
                'starring.runtime_slot_writer_fence_tenant_id_v2',
                'starring.runtime_slot_writer_fence_installation_id_v2',
                'starring.runtime_slot_writer_fence_deployment_id_v2',
                'starring.runtime_slot_writer_fence_expected_revision_v2',
                'starring.runtime_slot_writer_fence_marked_at_v2'
            ]) AS setting(setting_name)
            WHERE COALESCE(pg_catalog.current_setting(
                setting.setting_name,
                TRUE
            ), '') <> ''
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.drain_intent_id = requested_drain_intent_id
                AND drain.product_operation_id
                    = requested_product_operation_id
                AND drain.tenant_id = requested_tenant_id
                AND drain.installation_id = requested_installation_id
                AND drain.deployment_id = requested_deployment_id
                AND drain.slot_guild_id = requested_slot_guild_id
                AND drain.slot_ruleset_key = requested_slot_ruleset_key
                AND drain.expected_revision = requested_expected_revision
                AND drain.intent_state = 'pending'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_product_drain_state_invalid';
    END IF;

    mark_clock := pg_catalog.clock_timestamp();
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_action_v2',
        'mark_drain',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        requested_slot_guild_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        requested_slot_ruleset_key,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        requested_expected_epoch::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_drain_intent_id_v2',
        requested_drain_intent_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_product_operation_id_v2',
        requested_product_operation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_tenant_id_v2',
        requested_tenant_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_installation_id_v2',
        requested_installation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_deployment_id_v2',
        requested_deployment_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_expected_revision_v2',
        requested_expected_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_marked_at_v2',
        mark_clock::TEXT,
        TRUE
    );

    UPDATE public.runtime_slot_writer_fences_v2 AS fence
    SET writer_epoch = fence.writer_epoch + 1,
        pending_drain_intent_id = requested_drain_intent_id,
        pending_product_operation_id = requested_product_operation_id,
        pending_tenant_id = requested_tenant_id,
        pending_installation_id = requested_installation_id,
        pending_deployment_id = requested_deployment_id,
        pending_expected_revision = requested_expected_revision,
        pending_marked_at = mark_clock,
        updated_at = GREATEST(fence.updated_at, mark_clock)
    WHERE fence.slot_guild_id = requested_slot_guild_id
        AND fence.slot_ruleset_key = requested_slot_ruleset_key
        AND fence.writer_epoch = requested_expected_epoch
        AND fence.pending_drain_intent_id IS NULL
    RETURNING fence.writer_epoch INTO next_epoch;

    IF NOT FOUND THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_slot_writer_fence_action_v2',
            'starring.runtime_slot_writer_fence_slot_guild_id_v2',
            'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
            'starring.runtime_slot_writer_fence_expected_epoch_v2',
            'starring.runtime_slot_writer_fence_drain_intent_id_v2',
            'starring.runtime_slot_writer_fence_product_operation_id_v2',
            'starring.runtime_slot_writer_fence_tenant_id_v2',
            'starring.runtime_slot_writer_fence_installation_id_v2',
            'starring.runtime_slot_writer_fence_deployment_id_v2',
            'starring.runtime_slot_writer_fence_expected_revision_v2',
            'starring.runtime_slot_writer_fence_marked_at_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;

        SELECT fence.*
        INTO fence_row
        FROM public.runtime_slot_writer_fences_v2 AS fence
        WHERE fence.slot_guild_id = requested_slot_guild_id
            AND fence.slot_ruleset_key = requested_slot_ruleset_key;

        IF NOT FOUND
            OR fence_row.writer_epoch = 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_product_drain_state_invalid';
        ELSIF fence_row.pending_drain_intent_id IS NOT NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX007',
                MESSAGE = 'runtime_execution_product_drain_pending';
        END IF;

        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_stale';
    END IF;

    RETURN next_epoch;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
BEGIN
    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(
        NEW.discord_guild_id,
        NEW.ruleset_key
    );
    RETURN NEW;
END;
$function$;

CREATE TRIGGER automation_installations_create_runtime_slot_writer_fence_v2
AFTER INSERT ON public.automation_installations
FOR EACH ROW
EXECUTE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2();

REVOKE ALL PRIVILEGES ON TABLE
    public.runtime_slot_writer_fences_v2
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    public.reject_runtime_slot_writer_fence_mutation_v2()
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(
        TEXT,TEXT
    )
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
        TEXT,TEXT
    )
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
        TEXT,TEXT,BIGINT
    )
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(
        TEXT,TEXT,BIGINT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT
    )
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()
FROM PUBLIC;

DO $patch_first_apply$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
    );

    previous_fragment :=
        '    exception_constraint_name TEXT;' || E'\n' ||
        'BEGIN';
    next_fragment :=
        '    exception_constraint_name TEXT;' || E'\n' ||
        '    slot_fence_row RECORD;' || E'\n' ||
        'BEGIN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_first_apply_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment';
    next_fragment :=
        '    SELECT fence.*' || E'\n' ||
        '    INTO slot_fence_row' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        requested_slot_guild_id,' || E'\n' ||
        '        requested_slot_ruleset_key' || E'\n' ||
        '    ) AS fence;' || E'\n' ||
        E'\n' ||
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_first_apply_lock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF product_count = 1 THEN' || E'\n' ||
        '        stored_product_digest :=' || E'\n' ||
        '            starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(';
    next_fragment :=
        '    IF product_count = 1' || E'\n' ||
        '        AND (' || E'\n' ||
        '            slot_fence_row.pending_drain_intent_id' || E'\n' ||
        '                IS DISTINCT FROM drain_row.drain_intent_id' || E'\n' ||
        '            OR slot_fence_row.pending_product_operation_id' || E'\n' ||
        '                IS DISTINCT FROM drain_row.product_operation_id' || E'\n' ||
        '            OR slot_fence_row.pending_tenant_id' || E'\n' ||
        '                IS DISTINCT FROM drain_row.tenant_id' || E'\n' ||
        '            OR slot_fence_row.pending_installation_id' || E'\n' ||
        '                IS DISTINCT FROM drain_row.installation_id' || E'\n' ||
        '            OR slot_fence_row.pending_deployment_id' || E'\n' ||
        '                IS DISTINCT FROM drain_row.deployment_id' || E'\n' ||
        '            OR slot_fence_row.pending_expected_revision' || E'\n' ||
        '                IS DISTINCT FROM drain_row.expected_revision' || E'\n' ||
        '        )' || E'\n' ||
        '    THEN' || E'\n' ||
        '        outcome_name := ''persistence_corrupt'';' || E'\n' ||
        '        RETURN NEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    IF product_count = 1 THEN' || E'\n' ||
        '        stored_product_digest :=' || E'\n' ||
        '            starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_first_apply_pair_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF deployment_row.revision' || E'\n' ||
        '            IS DISTINCT FROM requested_expected_revision';
    next_fragment :=
        '    IF slot_fence_row.pending_drain_intent_id IS NOT NULL THEN' || E'\n' ||
        '        outcome_name := ''slot_conflict'';' || E'\n' ||
        '        locked_snapshot := NULL;' || E'\n' ||
        '        observed_at := NULL;' || E'\n' ||
        '        RETURN NEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    IF deployment_row.revision' || E'\n' ||
        '            IS DISTINCT FROM requested_expected_revision';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_first_apply_conflict_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    outcome_name := ''inserted'';' || E'\n' ||
        '    product_tenant_id := requested_tenant_id;';
    next_fragment :=
        '    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(' || E'\n' ||
        '        requested_slot_guild_id,' || E'\n' ||
        '        requested_slot_ruleset_key,' || E'\n' ||
        '        slot_fence_row.writer_epoch,' || E'\n' ||
        '        requested_intent_id,' || E'\n' ||
        '        requested_operation_id,' || E'\n' ||
        '        requested_tenant_id,' || E'\n' ||
        '        requested_installation_id,' || E'\n' ||
        '        requested_deployment_id,' || E'\n' ||
        '        requested_expected_revision' || E'\n' ||
        '    );' || E'\n' ||
        E'\n' ||
        '    outcome_name := ''inserted'';' || E'\n' ||
        '    product_tenant_id := requested_tenant_id;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_first_apply_mark_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_first_apply$;

CREATE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    current_drain_count BIGINT;
    current_fence_count BIGINT;
BEGIN
    IF TG_RELID = pg_catalog.to_regclass(
            'public.runtime_slot_writer_fences_v2'
        )
    THEN
        IF TG_OP = 'DELETE' THEN
            SELECT pg_catalog.count(*)
            INTO current_drain_count
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = OLD.slot_guild_id
                AND drain.slot_ruleset_key = OLD.slot_ruleset_key
                AND drain.intent_state = 'pending';
            IF current_drain_count <> 0 THEN
                RAISE EXCEPTION USING
                    ERRCODE = '23514',
                    MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
            END IF;
            RETURN NULL;
        END IF;

        IF NEW.pending_drain_intent_id IS NULL THEN
            SELECT pg_catalog.count(*)
            INTO current_drain_count
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = NEW.slot_guild_id
                AND drain.slot_ruleset_key = NEW.slot_ruleset_key
                AND drain.intent_state = 'pending';
            IF current_drain_count <> 0 THEN
                RAISE EXCEPTION USING
                    ERRCODE = '23514',
                    MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
            END IF;
            RETURN NULL;
        END IF;

        SELECT pg_catalog.count(*)
        INTO current_drain_count
        FROM public.runtime_drain_intents_v2 AS drain
        WHERE drain.drain_intent_id = NEW.pending_drain_intent_id
            AND drain.product_operation_id
                = NEW.pending_product_operation_id
            AND drain.tenant_id = NEW.pending_tenant_id
            AND drain.installation_id = NEW.pending_installation_id
            AND drain.deployment_id = NEW.pending_deployment_id
            AND drain.slot_guild_id = NEW.slot_guild_id
            AND drain.slot_ruleset_key = NEW.slot_ruleset_key
            AND drain.expected_revision = NEW.pending_expected_revision
            AND drain.intent_state = 'pending';
        IF current_drain_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
        END IF;
        RETURN NULL;
    END IF;

    IF TG_OP <> 'DELETE' AND NEW.intent_state = 'pending' THEN
        SELECT pg_catalog.count(*)
        INTO current_fence_count
        FROM public.runtime_slot_writer_fences_v2 AS fence
        WHERE fence.slot_guild_id = NEW.slot_guild_id
            AND fence.slot_ruleset_key = NEW.slot_ruleset_key
            AND fence.pending_drain_intent_id = NEW.drain_intent_id
            AND fence.pending_product_operation_id
                = NEW.product_operation_id
            AND fence.pending_tenant_id = NEW.tenant_id
            AND fence.pending_installation_id = NEW.installation_id
            AND fence.pending_deployment_id = NEW.deployment_id
            AND fence.pending_expected_revision = NEW.expected_revision;
        IF current_fence_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
        END IF;
    END IF;

    IF TG_OP <> 'INSERT'
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.drain_intent_id = OLD.drain_intent_id
                AND drain.intent_state = 'pending'
        )
    THEN
        SELECT pg_catalog.count(*)
        INTO current_fence_count
        FROM public.runtime_slot_writer_fences_v2 AS fence
        WHERE fence.pending_drain_intent_id = OLD.drain_intent_id;
        IF current_fence_count <> 0 THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
        END IF;
    END IF;

    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER runtime_slot_writer_fences_v2_assert_pending_symmetry
AFTER INSERT OR UPDATE OR DELETE ON public.runtime_slot_writer_fences_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2();

CREATE CONSTRAINT TRIGGER runtime_drain_intents_v2_assert_slot_writer_fence_symmetry
AFTER INSERT OR UPDATE OR DELETE ON public.runtime_drain_intents_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2();

REVOKE ALL PRIVILEGES ON FUNCTION
    public.validate_runtime_slot_writer_fence_symmetry_v2()
FROM PUBLIC;

DO $patch_shared_manifests$
DECLARE
    definition TEXT;
    expected RECORD;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    FOR expected IN
        SELECT manifest.*
        FROM (
            VALUES
                (
                    'public.starring_runtime_exact_target_schema_manifest_v1()',
                    354::BIGINT,
                    'cfe8dc4188faae09cb70e99a6f91e6ae3921cfcd0983d85ed42e2649b0cb1d4d'::TEXT,
                    356::BIGINT,
                    'ca4d76873d9256406baaad080943a78b7a6eeeae409ad67e8dc896f0a237642a'::TEXT
                ),
                (
                    'public.starring_runtime_serving_schema_manifest_v1()',
                    469::BIGINT,
                    '7ef840eba14126d4dae1d05ae4920858f7b72d1b4fb4f14d8477abdc65d982ea'::TEXT,
                    471::BIGINT,
                    '1b476578005a17dadfa9a6f3d26f966e929af5909cdc5097eb6a63050ec310fa'::TEXT
                )
        ) AS manifest(
            identity,
            previous_count,
            previous_digest,
            next_count,
            next_digest
        )
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            expected.identity
        );

        previous_fragment :=
            '    RETURN observed_count = ' || expected.previous_count || E'\n' ||
            '        AND observed_digest' || E'\n' ||
            '            = ' || pg_catalog.quote_literal(
                expected.previous_digest
            ) || ';';
        next_fragment :=
            '    RETURN observed_count = ' || expected.next_count || E'\n' ||
            '        AND observed_digest' || E'\n' ||
            '            = ' || pg_catalog.quote_literal(
                expected.next_digest
            ) || ';';
        IF definition IS NULL
            OR pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_slot_writer_fence_shared_manifest_drift';
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;
END;
$patch_shared_manifests$;

DO $patch_shared_readiness$
DECLARE
    definition TEXT;
    expected RECORD;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    FOR expected IN
        SELECT readiness.*
        FROM (
            VALUES
                (
                    'public.starring_runtime_exact_target_database_readiness_v1()',
                    '5b705cf2cd0fd7562d04663a6984259b33d36ee66cd5689159f11c44d0632b83'::TEXT,
                    '5fe0365d0cb4912a01778f3d30a2d649a40e82c5b964ba9e2e7e1901e79eb109'::TEXT
                ),
                (
                    'public.starring_runtime_serving_database_readiness_v1()',
                    '133f73f8eb70606e023af29294ade8bb593b2adc06db3e663bdd42d7693a43be'::TEXT,
                    '14a0c119d8fa0b7a85b72509df29156a6c869b5e3f240bc8fffc89fd1a86c4c9'::TEXT
                )
        ) AS readiness(identity, previous_digest, next_digest)
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            expected.identity
        );

        previous_fragment :=
            pg_catalog.quote_literal(expected.previous_digest) || '::TEXT';
        next_fragment :=
            pg_catalog.quote_literal(expected.next_digest) || '::TEXT';
        IF definition IS NULL
            OR pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_slot_writer_fence_shared_readiness_drift';
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;
END;
$patch_shared_readiness$;

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

    previous_fragment :=
        '            (pg_catalog.to_regclass(''public.runtime_drain_intents_v2'')),' || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_attestations'')),';
    next_fragment :=
        '            (pg_catalog.to_regclass(''public.runtime_drain_intents_v2'')),' || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_slot_writer_fences_v2'')),' || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_attestations'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_manifest_relation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.reject_runtime_slot_writer_fence_mutation_v2()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.validate_runtime_slot_writer_fence_symmetry_v2()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_manifest_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RETURN observed_count = 582' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''cfeaad4271e12f72f20aa57ad8dc92c63a787f260551fee414897a69143b20de'';';
    next_fragment :=
        '    RETURN observed_count = 623' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''ce1e493041abc52b6f4073da976a99b547b32a92d7ff171b64eef791354ff491'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_manifest_expectation_drift';
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
        '            (''public.runtime_drain_intents_v2''),' || E'\n' ||
        '            (''public.runtime_attestations''),';
    next_fragment :=
        '            (''public.runtime_drain_intents_v2''),' || E'\n' ||
        '            (''public.runtime_slot_writer_fences_v2''),' || E'\n' ||
        '            (''public.runtime_attestations''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_readiness_relation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    next_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''),' || E'\n' ||
        '            (''public.reject_runtime_slot_writer_fence_mutation_v2()''),' || E'\n' ||
        '            (''public.validate_runtime_slot_writer_fence_symmetry_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_readiness_protected_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'')' || E'\n' ||
        '    ) AS expected(identity)' || E'\n' ||
        '    LEFT JOIN pg_catalog.pg_proc AS function_row' || E'\n' ||
        '        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)' || E'\n' ||
        '    WHERE function_row.oid IS NULL' || E'\n' ||
        '        OR (' || E'\n' ||
        '            SELECT pg_catalog.count(*)';
    next_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()'')' || E'\n' ||
        '    ) AS expected(identity)' || E'\n' ||
        '    LEFT JOIN pg_catalog.pg_proc AS function_row' || E'\n' ||
        '        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)' || E'\n' ||
        '    WHERE function_row.oid IS NULL' || E'\n' ||
        '        OR (' || E'\n' ||
        '            SELECT pg_catalog.count(*)';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_readiness_private_acl_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''331a95180a75109385566b0b1b0659e247e5619cf02e2f61ee89904a2751856b''::TEXT';
    next_fragment :=
        '''223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_readiness_manifest_digest_drift';
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
    invalid_relation_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_index_count BIGINT;
    invalid_trigger_count BIGINT;
    invalid_function_count BIGINT;
    invalid_acl_count BIGINT;
    installation_count BIGINT;
    fence_count BIGINT;
    unmatched_installation_count BIGINT;
    unmatched_fence_count BIGINT;
    exact_target_manifest_digest TEXT;
    exact_target_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    core_definition TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.automation_installations'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.automation_installations'),
            ('public.runtime_product_operations_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_slot_writer_fences_v2')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            ('public.runtime_drain_intents_v2',
                'runtime_drain_intents_v2_fence_identity_unique', 'u'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_pkey', 'p'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_installation_fk', 'f'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_pending_fk', 'f'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_pending_intent_unique', 'u'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_pending_product_unique', 'u'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_slot_check', 'c'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_epoch_check', 'c'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_pending_shape_check', 'c'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_updated_at_check', 'c')
    ) AS expected(relation_identity, constraint_name, constraint_type)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = pg_catalog.to_regclass(
            expected.relation_identity
        )
        AND constraint_row.conname = expected.constraint_name
    WHERE constraint_row.oid IS NULL
        OR constraint_row.contype::TEXT <> expected.constraint_type
        OR NOT constraint_row.convalidated
        OR constraint_row.conparentid <> 0;

    SELECT pg_catalog.count(*)
    INTO invalid_index_count
    FROM pg_catalog.pg_index AS index_row
    INNER JOIN pg_catalog.pg_class AS index_relation
        ON index_relation.oid = index_row.indexrelid
    WHERE index_row.indrelid = pg_catalog.to_regclass(
            'public.runtime_drain_intents_v2'
        )
        AND index_relation.relname
            = 'runtime_drain_intents_v2_one_pending_per_slot'
        AND (
            NOT index_row.indisunique
            OR NOT index_row.indisvalid
            OR NOT index_row.indisready
            OR index_row.indpred IS NULL
        );
    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_index AS index_row
        INNER JOIN pg_catalog.pg_class AS index_relation
            ON index_relation.oid = index_row.indexrelid
        WHERE index_row.indrelid = pg_catalog.to_regclass(
                'public.runtime_drain_intents_v2'
            )
            AND index_relation.relname
                = 'runtime_drain_intents_v2_one_pending_per_slot'
    ) THEN
        invalid_index_count := invalid_index_count + 1;
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_reject_row_mutation',
                'public.reject_runtime_slot_writer_fence_mutation_v2()'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_reject_truncate',
                'public.reject_runtime_slot_writer_fence_mutation_v2()'),
            ('public.runtime_slot_writer_fences_v2',
                'runtime_slot_writer_fences_v2_assert_pending_symmetry',
                'public.validate_runtime_slot_writer_fence_symmetry_v2()'),
            ('public.runtime_drain_intents_v2',
                'runtime_drain_intents_v2_assert_slot_writer_fence_symmetry',
                'public.validate_runtime_slot_writer_fence_symmetry_v2()'),
            ('public.automation_installations',
                'automation_installations_create_runtime_slot_writer_fence_v2',
                'starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()')
    ) AS expected(relation_identity, trigger_name, function_identity)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            expected.relation_identity
        )
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(
            expected.function_identity
        )
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgisinternal
        OR trigger_row.tgparentid <> 0;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.reject_runtime_slot_writer_fence_mutation_v2()',
                FALSE, FALSE, 'trigger'),
            ('public.validate_runtime_slot_writer_fence_symmetry_v2()',
                TRUE, FALSE, 'trigger'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(text,text)',
                FALSE, TRUE, 'bigint'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                FALSE, TRUE,
                'TABLE(writer_epoch bigint, pending_drain_intent_id text, pending_product_operation_id text, pending_tenant_id text, pending_installation_id text, pending_deployment_id text, pending_expected_revision bigint, pending_marked_at timestamp with time zone, observed_at timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)',
                FALSE, TRUE, 'bigint'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)',
                FALSE, TRUE, 'bigint'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()',
                FALSE, FALSE, 'trigger')
    ) AS expected(identity, security_definer, is_strict, result_name)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proisstrict <> expected.is_strict
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            <> ARRAY['search_path=pg_catalog']::TEXT[]
        OR language_row.lanname <> 'plpgsql'
        OR pg_catalog.pg_get_function_result(function_row.oid)
            <> expected.result_name;

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM (
        VALUES
            ('public.reject_runtime_slot_writer_fence_mutation_v2()'),
            ('public.validate_runtime_slot_writer_fence_symmetry_v2()'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_create_v2(text,text)'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()')
    ) AS expected(identity)
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner
        OR privilege.grantor <> common_owner
        OR privilege.privilege_type <> 'EXECUTE'
        OR privilege.is_grantable;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.aclexplode(COALESCE(
            (
                SELECT relation.relacl
                FROM pg_catalog.pg_class AS relation
                WHERE relation.oid = pg_catalog.to_regclass(
                    'public.runtime_slot_writer_fences_v2'
                )
            ),
            pg_catalog.acldefault('r', common_owner)
        )) AS privilege
        WHERE privilege.grantee <> common_owner
            OR privilege.grantor <> common_owner
            OR privilege.is_grantable
    ) THEN
        invalid_acl_count := invalid_acl_count + 1;
    END IF;

    SELECT pg_catalog.count(*)
    INTO installation_count
    FROM public.automation_installations;
    SELECT pg_catalog.count(*)
    INTO fence_count
    FROM public.runtime_slot_writer_fences_v2;
    SELECT pg_catalog.count(*)
    INTO unmatched_installation_count
    FROM public.automation_installations AS installation
    LEFT JOIN public.runtime_slot_writer_fences_v2 AS fence
        ON fence.slot_guild_id = installation.discord_guild_id
        AND fence.slot_ruleset_key = installation.ruleset_key
    WHERE fence.slot_guild_id IS NULL;
    SELECT pg_catalog.count(*)
    INTO unmatched_fence_count
    FROM public.runtime_slot_writer_fences_v2 AS fence
    LEFT JOIN public.automation_installations AS installation
        ON installation.discord_guild_id = fence.slot_guild_id
        AND installation.ruleset_key = fence.slot_ruleset_key
    WHERE installation.installation_id IS NULL;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO exact_target_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO exact_target_readiness_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO serving_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO serving_readiness_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO readiness_digest;
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO core_definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
    );

    IF common_owner IS NULL
        OR invalid_relation_count <> 0
        OR invalid_constraint_count <> 0
        OR invalid_index_count <> 0
        OR invalid_trigger_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR installation_count <> fence_count
        OR unmatched_installation_count <> 0
        OR unmatched_fence_count <> 0
        OR EXISTS (
            SELECT 1
            FROM public.runtime_product_operations_v2
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2
        )
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR exact_target_manifest_digest
            <> '5fe0365d0cb4912a01778f3d30a2d649a40e82c5b964ba9e2e7e1901e79eb109'
        OR exact_target_readiness_digest
            <> 'e4bae4b38acc529accd4401af853eb7e96d2a34ad8fb1224b9965166ff40c229'
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR serving_manifest_digest
            <> '14a0c119d8fa0b7a85b72509df29156a6c869b5e3f240bc8fffc89fd1a86c4c9'
        OR serving_readiness_digest
            <> '1c0c79c6fbf528f28fb56e91a54b78cd1fe17c70d2bc3e8d7e3dc515d8a7f8f7'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR manifest_digest
            <> '223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292'
        OR readiness_digest
            <> '48a10f783603fe02879f2a1cddbecbb39541ac0ca154c77f7b1e0eef8d9f6834'
        OR pg_catalog.strpos(
            core_definition,
            'starring_runtime_slot_writer_fence_lock_v2'
        ) = 0
        OR pg_catalog.strpos(
            core_definition,
            'starring_runtime_slot_writer_fence_mark_drain_v2'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_writer_fence_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
