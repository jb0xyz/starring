SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_deployments,
    public.activation_requests,
    public.authoring_promotions,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS SHARE MODE;

LOCK TABLE
    public.ruleset_panel_installations,
    public.strict_panel_operation_journal
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    persistent_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
    unsafe_default_count BIGINT;
    helper_oid OID;
    helper_acl_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relpersistence = 'p'),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, persistent_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.ruleset_panel_installations')),
            (pg_catalog.to_regclass('public.strict_panel_operation_journal'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF relation_count <> 10
        OR ordinary_count <> 10
        OR persistent_count <> 10
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'runtime panel relations require one persistent ordinary-table owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'runtime panel migration requires the common owner'
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
        AND defaults.defaclrole = common_owner
        AND privilege.grantee <> defaults.defaclrole;

    IF unsafe_schema_create_count <> 0 OR unsafe_default_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel schema trust is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_panel_execution_lock_v1',
            'starring_runtime_panel_reconciliation_lock_v1',
            'starring_runtime_panel_reconciliation_claim_v1',
            'starring_runtime_panel_reconciliation_check_v1',
            'starring_runtime_panel_reconciliation_snapshot_v1',
            'starring_runtime_panel_reconciliation_installation_upsert_v1',
            'starring_runtime_panel_reconciliation_installation_remove_v1',
            'starring_runtime_panel_reconciliation_journal_put_v1',
            'starring_runtime_panel_reconciliation_journal_remove_v1'
        );

    IF collision_count <> 0
        OR pg_catalog.to_regclass('public.runtime_panel_reconciliation_sessions') IS NOT NULL
        OR pg_catalog.to_regclass('public.runtime_panel_reconciliation_session_id_unique') IS NOT NULL
    THEN
        RAISE EXCEPTION 'runtime panel schema collides with an existing object'
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
        AND relation.relname IN (
            'ruleset_panel_installations',
            'strict_panel_operation_journal'
        )
        AND attribute.attname IN (
            'record_revision',
            'last_reconciliation_session_id',
            'last_runtime_deployment_id',
            'last_runtime_deployment_revision'
        )
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel provenance columns already exist'
            USING ERRCODE = '55000';
    END IF;

    helper_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'
    );
    IF helper_oid IS NULL THEN
        RAISE EXCEPTION 'runtime panel authority helper is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO helper_acl_count
    FROM pg_catalog.aclexplode(COALESCE(
        (SELECT function_row.proacl
         FROM pg_catalog.pg_proc AS function_row
         WHERE function_row.oid = helper_oid),
        pg_catalog.acldefault('f', common_owner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_language AS language_row
            ON language_row.oid = function_row.prolang
        WHERE function_row.oid = helper_oid
            AND function_row.proowner = common_owner
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND NOT function_row.proisstrict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_result(function_row.oid) = 'text'
    ) OR helper_acl_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel authority helper contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM (
            SELECT installation.guild_id,
                installation.ruleset_key,
                installation.panel_key
            FROM public.ruleset_panel_installations AS installation
            UNION
            SELECT journal.guild_id,
                journal.ruleset_key,
                journal.panel_key
            FROM public.strict_panel_operation_journal AS journal
        ) AS resident
        GROUP BY resident.guild_id, resident.ruleset_key
        HAVING pg_catalog.count(*) > 256
    ) THEN
        RAISE EXCEPTION 'legacy runtime panel slot exceeds bounded capacity'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

ALTER TABLE public.ruleset_panel_installations
ADD COLUMN record_revision BIGINT NOT NULL DEFAULT 1,
ADD COLUMN last_reconciliation_session_id TEXT,
ADD COLUMN last_runtime_deployment_id TEXT,
ADD COLUMN last_runtime_deployment_revision BIGINT;

ALTER TABLE public.ruleset_panel_installations
ADD CONSTRAINT ruleset_panel_installations_runtime_provenance_valid CHECK (
    record_revision BETWEEN 1 AND 9223372036854775807
    AND (
        (
            last_reconciliation_session_id IS NULL
            AND last_runtime_deployment_id IS NULL
            AND last_runtime_deployment_revision IS NULL
        )
        OR (
            last_reconciliation_session_id ~ '^[0-9a-f]{64}$'
            AND last_runtime_deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND last_runtime_deployment_revision BETWEEN 1 AND 9223372036854775807
        )
    )
) NOT VALID;

ALTER TABLE public.strict_panel_operation_journal
ADD COLUMN record_revision BIGINT NOT NULL DEFAULT 1,
ADD COLUMN last_reconciliation_session_id TEXT,
ADD COLUMN last_runtime_deployment_id TEXT,
ADD COLUMN last_runtime_deployment_revision BIGINT;

ALTER TABLE public.strict_panel_operation_journal
ADD CONSTRAINT strict_panel_operation_journal_runtime_provenance_valid CHECK (
    record_revision BETWEEN 1 AND 9223372036854775807
    AND (
        (
            last_reconciliation_session_id IS NULL
            AND last_runtime_deployment_id IS NULL
            AND last_runtime_deployment_revision IS NULL
        )
        OR (
            last_reconciliation_session_id ~ '^[0-9a-f]{64}$'
            AND last_runtime_deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND last_runtime_deployment_revision BETWEEN 1 AND 9223372036854775807
        )
    )
) NOT VALID;

ALTER TABLE public.ruleset_panel_installations
VALIDATE CONSTRAINT ruleset_panel_installations_runtime_provenance_valid;

ALTER TABLE public.strict_panel_operation_journal
VALIDATE CONSTRAINT strict_panel_operation_journal_runtime_provenance_valid;

CREATE TABLE public.runtime_panel_reconciliation_sessions (
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    controller_id TEXT NOT NULL,
    controller_fencing_token BIGINT NOT NULL,
    convergence_attempt_no BIGINT NOT NULL,
    runtime_generation BIGINT NOT NULL,
    target_version BIGINT NOT NULL,
    target_content_hash TEXT NOT NULL,
    binding_revision BIGINT NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    installation_authority_revision BIGINT NOT NULL,
    current_authority_revision BIGINT NOT NULL,
    session_id TEXT NOT NULL,
    session_record_revision BIGINT NOT NULL,
    next_record_revision BIGINT NOT NULL,
    controller_lease_expires_at TIMESTAMPTZ NOT NULL,
    claimed_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key),
    CONSTRAINT runtime_panel_reconciliation_sessions_deployment_fk
        FOREIGN KEY (deployment_id)
        REFERENCES public.runtime_deployments (deployment_id)
        ON DELETE RESTRICT,
    CONSTRAINT runtime_panel_reconciliation_sessions_slot_valid CHECK (
        CASE
            WHEN guild_id ~ '^[1-9][0-9]{0,19}$'
            THEN guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
        AND (ruleset_key COLLATE "C") ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT runtime_panel_reconciliation_sessions_scope_valid CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_revision BETWEEN 1 AND 9223372036854775807
        AND controller_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND controller_fencing_token BETWEEN 1 AND 9223372036854775807
        AND convergence_attempt_no BETWEEN 1 AND 4294967295
        AND runtime_generation BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_panel_reconciliation_sessions_target_valid CHECK (
        target_version BETWEEN 1 AND 4294967295
        AND target_content_hash ~ '^[0-9a-f]{64}$'
        AND binding_revision BETWEEN 1 AND 9223372036854775807
        AND binding_fingerprint ~ '^[0-9a-f]{64}$'
        AND installation_authority_revision BETWEEN 1 AND 9223372036854775807
        AND current_authority_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_panel_reconciliation_sessions_identity_valid CHECK (
        session_id ~ '^[0-9a-f]{64}$'
        AND session_record_revision BETWEEN 1 AND 9223372036854775807
        AND next_record_revision BETWEEN 2 AND 9223372036854775807
    ),
    CONSTRAINT runtime_panel_reconciliation_sessions_time_valid CHECK (
        claimed_at <= updated_at
        AND claimed_at < controller_lease_expires_at
    )
);

CREATE UNIQUE INDEX runtime_panel_reconciliation_session_id_unique
ON public.runtime_panel_reconciliation_sessions (session_id COLLATE "C");

CREATE FUNCTION public.starring_runtime_panel_execution_lock_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT
)
RETURNS TABLE(
    checked_at TIMESTAMPTZ,
    controller_lease_expires_at TIMESTAMPTZ
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
    authority_status TEXT;
    persisted_current_authority_revision BIGINT;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR NOT (CASE
            WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
            THEN expected_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR (expected_ruleset_key COLLATE "C") !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_installation_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_current_authority_revision NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_request';
    END IF;

    SELECT *
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    checked_at := pg_catalog.clock_timestamp();

    IF NOT FOUND
        OR deployment_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR deployment_row.installation_id IS DISTINCT FROM expected_installation_id
        OR deployment_row.deployment_id IS DISTINCT FROM expected_deployment_id
        OR deployment_row.revision IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.controller_id IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.runtime_generation IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.guild_id IS DISTINCT FROM expected_guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR deployment_row.target_version IS DISTINCT FROM expected_target_version
        OR deployment_row.target_content_hash IS DISTINCT FROM expected_target_content_hash
        OR deployment_row.binding_revision IS DISTINCT FROM expected_binding_revision
        OR deployment_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
        OR deployment_row.installation_authority_revision
            IS DISTINCT FROM expected_installation_authority_revision
        OR deployment_row.phase <> 'reconciling_panels'
        OR deployment_row.blocked_at IS NOT NULL
        OR deployment_row.controller_acquired_at IS NULL
        OR deployment_row.controller_acquired_at > checked_at
        OR deployment_row.controller_lease_expires_at IS NULL
        OR deployment_row.controller_lease_expires_at <= checked_at
        OR (
            deployment_row.next_retry_at IS NOT NULL
            AND deployment_row.next_retry_at > checked_at
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP001',
            MESSAGE = 'runtime_panel_ownership_lost';
    END IF;

    authority_status := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        expected_tenant_id,
        expected_installation_id,
        expected_installation_authority_revision,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint
    );

    IF authority_status IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP005',
            MESSAGE = 'runtime_panel_authority_changed';
    END IF;

    SELECT installation.current_authority_revision
    INTO persisted_current_authority_revision
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id;

    IF NOT FOUND
        OR persisted_current_authority_revision
            IS DISTINCT FROM expected_current_authority_revision
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP005',
            MESSAGE = 'runtime_panel_authority_changed';
    END IF;

    controller_lease_expires_at := deployment_row.controller_lease_expires_at;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_snapshot_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT
)
RETURNS TABLE(
    record_kind TEXT,
    record_revision BIGINT,
    record_format_version SMALLINT,
    guild_id TEXT,
    ruleset_key TEXT,
    panel_key TEXT,
    installed_version BIGINT,
    channel_id TEXT,
    message_id TEXT,
    spec_hash TEXT,
    state_tag TEXT,
    operation_payload JSONB
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 512
AS $function$
DECLARE
    resident_count BIGINT;
BEGIN
    PERFORM *
    FROM public.starring_runtime_panel_reconciliation_lock_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_controller_id,
        expected_controller_fencing_token,
        expected_convergence_attempt_no,
        expected_runtime_generation,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision,
        expected_session_id,
        expected_session_record_revision
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    SELECT pg_catalog.count(*)
    INTO resident_count
    FROM (
        SELECT installation.panel_key
        FROM public.ruleset_panel_installations AS installation
        WHERE installation.guild_id = expected_guild_id
            AND installation.ruleset_key = expected_ruleset_key
        UNION
        SELECT journal.panel_key
        FROM public.strict_panel_operation_journal AS journal
        WHERE journal.guild_id = expected_guild_id
            AND journal.ruleset_key = expected_ruleset_key
    ) AS resident;

    IF resident_count > 256 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    RETURN QUERY
    SELECT snapshot.record_kind,
        snapshot.record_revision,
        snapshot.record_format_version,
        snapshot.guild_id,
        snapshot.ruleset_key,
        snapshot.panel_key,
        snapshot.installed_version,
        snapshot.channel_id,
        snapshot.message_id,
        snapshot.spec_hash,
        snapshot.state_tag,
        snapshot.operation_payload
    FROM (
        SELECT 'installation'::TEXT AS record_kind,
            installation.record_revision,
            NULL::SMALLINT AS record_format_version,
            installation.guild_id,
            installation.ruleset_key,
            installation.panel_key,
            installation.installed_version,
            installation.channel_id,
            installation.message_id,
            installation.spec_hash,
            NULL::TEXT AS state_tag,
            NULL::JSONB AS operation_payload
        FROM public.ruleset_panel_installations AS installation
        WHERE installation.guild_id = expected_guild_id
            AND installation.ruleset_key = expected_ruleset_key
        UNION ALL
        SELECT 'journal'::TEXT AS record_kind,
            journal.record_revision,
            journal.record_format_version,
            journal.guild_id,
            journal.ruleset_key,
            journal.panel_key,
            NULL::BIGINT AS installed_version,
            NULL::TEXT AS channel_id,
            NULL::TEXT AS message_id,
            NULL::TEXT AS spec_hash,
            journal.state_tag,
            journal.operation_payload
        FROM public.strict_panel_operation_journal AS journal
        WHERE journal.guild_id = expected_guild_id
            AND journal.ruleset_key = expected_ruleset_key
    ) AS snapshot
    ORDER BY snapshot.panel_key COLLATE "C", snapshot.record_kind COLLATE "C";
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_lock_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT
)
RETURNS TABLE(
    checked_at TIMESTAMPTZ,
    controller_lease_expires_at TIMESTAMPTZ
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
    execution_row RECORD;
    session_row public.runtime_panel_reconciliation_sessions%ROWTYPE;
    persisted_max_revision BIGINT;
BEGIN
    IF expected_session_id !~ '^[0-9a-f]{64}$'
        OR expected_session_record_revision NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_request';
    END IF;

    SELECT *
    INTO execution_row
    FROM public.starring_runtime_panel_execution_lock_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_controller_id,
        expected_controller_fencing_token,
        expected_convergence_attempt_no,
        expected_runtime_generation,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    SELECT *
    INTO session_row
    FROM public.runtime_panel_reconciliation_sessions AS session
    WHERE session.guild_id = expected_guild_id
        AND session.ruleset_key = expected_ruleset_key
    FOR UPDATE;

    IF NOT FOUND
        OR session_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR session_row.installation_id IS DISTINCT FROM expected_installation_id
        OR session_row.deployment_id IS DISTINCT FROM expected_deployment_id
        OR session_row.deployment_revision IS DISTINCT FROM expected_deployment_revision
        OR session_row.controller_id IS DISTINCT FROM expected_controller_id
        OR session_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR session_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR session_row.runtime_generation IS DISTINCT FROM expected_runtime_generation
        OR session_row.target_version IS DISTINCT FROM expected_target_version
        OR session_row.target_content_hash IS DISTINCT FROM expected_target_content_hash
        OR session_row.binding_revision IS DISTINCT FROM expected_binding_revision
        OR session_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
        OR session_row.installation_authority_revision
            IS DISTINCT FROM expected_installation_authority_revision
        OR session_row.current_authority_revision
            IS DISTINCT FROM expected_current_authority_revision
        OR session_row.session_id IS DISTINCT FROM expected_session_id
        OR session_row.session_record_revision
            IS DISTINCT FROM expected_session_record_revision
        OR session_row.controller_lease_expires_at
            IS DISTINCT FROM execution_row.controller_lease_expires_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP001',
            MESSAGE = 'runtime_panel_session_ownership_lost';
    END IF;

    IF session_row.next_record_revision NOT BETWEEN 2 AND 9223372036854775807 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    SELECT pg_catalog.greatest(
        COALESCE((
            SELECT pg_catalog.max(installation.record_revision)
            FROM public.ruleset_panel_installations AS installation
            WHERE installation.guild_id = expected_guild_id
                AND installation.ruleset_key = expected_ruleset_key
        ), 0),
        COALESCE((
            SELECT pg_catalog.max(journal.record_revision)
            FROM public.strict_panel_operation_journal AS journal
            WHERE journal.guild_id = expected_guild_id
                AND journal.ruleset_key = expected_ruleset_key
        ), 0)
    )
    INTO persisted_max_revision;

    IF session_row.next_record_revision <= persisted_max_revision THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    checked_at := execution_row.checked_at;
    controller_lease_expires_at := execution_row.controller_lease_expires_at;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_claim_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    requested_session_id TEXT
)
RETURNS TABLE(
    session_record_revision BIGINT,
    checked_at TIMESTAMPTZ,
    controller_lease_expires_at TIMESTAMPTZ
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
    execution_row RECORD;
    session_row public.runtime_panel_reconciliation_sessions%ROWTYPE;
    legacy_max_revision BIGINT;
    replacement_revision BIGINT;
    persisted_max_revision BIGINT;
BEGIN
    IF requested_session_id !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_request';
    END IF;

    SELECT *
    INTO execution_row
    FROM public.starring_runtime_panel_execution_lock_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_controller_id,
        expected_controller_fencing_token,
        expected_convergence_attempt_no,
        expected_runtime_generation,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    SELECT *
    INTO session_row
    FROM public.runtime_panel_reconciliation_sessions AS session
    WHERE session.guild_id = expected_guild_id
        AND session.ruleset_key = expected_ruleset_key
    FOR UPDATE;

    IF FOUND THEN
        SELECT pg_catalog.greatest(
            COALESCE((
                SELECT pg_catalog.max(installation.record_revision)
                FROM public.ruleset_panel_installations AS installation
                WHERE installation.guild_id = expected_guild_id
                    AND installation.ruleset_key = expected_ruleset_key
            ), 0),
            COALESCE((
                SELECT pg_catalog.max(journal.record_revision)
                FROM public.strict_panel_operation_journal AS journal
                WHERE journal.guild_id = expected_guild_id
                    AND journal.ruleset_key = expected_ruleset_key
            ), 0)
        )
        INTO persisted_max_revision;

        IF session_row.next_record_revision <= persisted_max_revision THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_persistence_corrupt';
        END IF;

        IF session_row.tenant_id IS NOT DISTINCT FROM expected_tenant_id
            AND session_row.installation_id IS NOT DISTINCT FROM expected_installation_id
            AND session_row.deployment_id IS NOT DISTINCT FROM expected_deployment_id
            AND session_row.deployment_revision IS NOT DISTINCT FROM expected_deployment_revision
            AND session_row.controller_id IS NOT DISTINCT FROM expected_controller_id
            AND session_row.controller_fencing_token
                IS NOT DISTINCT FROM expected_controller_fencing_token
            AND session_row.convergence_attempt_no
                IS NOT DISTINCT FROM expected_convergence_attempt_no
            AND session_row.runtime_generation IS NOT DISTINCT FROM expected_runtime_generation
            AND session_row.target_version IS NOT DISTINCT FROM expected_target_version
            AND session_row.target_content_hash IS NOT DISTINCT FROM expected_target_content_hash
            AND session_row.binding_revision IS NOT DISTINCT FROM expected_binding_revision
            AND session_row.binding_fingerprint IS NOT DISTINCT FROM expected_binding_fingerprint
            AND session_row.installation_authority_revision
                IS NOT DISTINCT FROM expected_installation_authority_revision
            AND session_row.current_authority_revision
                IS NOT DISTINCT FROM expected_current_authority_revision
        THEN
            IF session_row.session_id IS DISTINCT FROM requested_session_id THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RP002',
                    MESSAGE = 'runtime_panel_session_busy';
            END IF;
            IF session_row.controller_lease_expires_at
                IS DISTINCT FROM execution_row.controller_lease_expires_at
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RP001',
                    MESSAGE = 'runtime_panel_ownership_lost';
            END IF;
            session_record_revision := session_row.session_record_revision;
            checked_at := execution_row.checked_at;
            controller_lease_expires_at := execution_row.controller_lease_expires_at;
            RETURN NEXT;
            RETURN;
        END IF;

        IF session_row.session_record_revision = 9223372036854775807
            OR session_row.next_record_revision NOT BETWEEN 2 AND 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_revision_exhausted';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM public.runtime_panel_reconciliation_sessions AS other_session
            WHERE other_session.session_id = requested_session_id
                AND (
                    other_session.guild_id <> expected_guild_id
                    OR other_session.ruleset_key <> expected_ruleset_key
                )
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_session_busy';
        END IF;

        replacement_revision := session_row.session_record_revision + 1;
        UPDATE public.runtime_panel_reconciliation_sessions AS session
        SET tenant_id = expected_tenant_id,
            installation_id = expected_installation_id,
            deployment_id = expected_deployment_id,
            deployment_revision = expected_deployment_revision,
            controller_id = expected_controller_id,
            controller_fencing_token = expected_controller_fencing_token,
            convergence_attempt_no = expected_convergence_attempt_no,
            runtime_generation = expected_runtime_generation,
            target_version = expected_target_version,
            target_content_hash = expected_target_content_hash,
            binding_revision = expected_binding_revision,
            binding_fingerprint = expected_binding_fingerprint,
            installation_authority_revision = expected_installation_authority_revision,
            current_authority_revision = expected_current_authority_revision,
            session_id = requested_session_id,
            session_record_revision = replacement_revision,
            controller_lease_expires_at = execution_row.controller_lease_expires_at,
            claimed_at = execution_row.checked_at,
            updated_at = execution_row.checked_at
        WHERE session.guild_id = expected_guild_id
            AND session.ruleset_key = expected_ruleset_key;

        session_record_revision := replacement_revision;
        checked_at := execution_row.checked_at;
        controller_lease_expires_at := execution_row.controller_lease_expires_at;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.greatest(
        COALESCE((
            SELECT pg_catalog.max(installation.record_revision)
            FROM public.ruleset_panel_installations AS installation
            WHERE installation.guild_id = expected_guild_id
                AND installation.ruleset_key = expected_ruleset_key
        ), 1),
        COALESCE((
            SELECT pg_catalog.max(journal.record_revision)
            FROM public.strict_panel_operation_journal AS journal
            WHERE journal.guild_id = expected_guild_id
                AND journal.ruleset_key = expected_ruleset_key
        ), 1)
    )
    INTO legacy_max_revision;

    IF legacy_max_revision NOT BETWEEN 1 AND 9223372036854775806 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_revision_exhausted';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.runtime_panel_reconciliation_sessions AS other_session
        WHERE other_session.session_id = requested_session_id
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP002',
            MESSAGE = 'runtime_panel_session_busy';
    END IF;

    INSERT INTO public.runtime_panel_reconciliation_sessions (
        guild_id,
        ruleset_key,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        controller_id,
        controller_fencing_token,
        convergence_attempt_no,
        runtime_generation,
        target_version,
        target_content_hash,
        binding_revision,
        binding_fingerprint,
        installation_authority_revision,
        current_authority_revision,
        session_id,
        session_record_revision,
        next_record_revision,
        controller_lease_expires_at,
        claimed_at,
        updated_at
    ) VALUES (
        expected_guild_id,
        expected_ruleset_key,
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_controller_id,
        expected_controller_fencing_token,
        expected_convergence_attempt_no,
        expected_runtime_generation,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision,
        requested_session_id,
        1,
        legacy_max_revision + 1,
        execution_row.controller_lease_expires_at,
        execution_row.checked_at,
        execution_row.checked_at
    );

    session_record_revision := 1;
    checked_at := execution_row.checked_at;
    controller_lease_expires_at := execution_row.controller_lease_expires_at;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_check_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT,
    required_lease_headroom_ms BIGINT
)
RETURNS TABLE(
    checked_at TIMESTAMPTZ,
    controller_lease_expires_at TIMESTAMPTZ
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
    locked_row RECORD;
BEGIN
    IF required_lease_headroom_ms NOT BETWEEN 1 AND 30000 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_lease_headroom';
    END IF;

    SELECT *
    INTO locked_row
    FROM public.starring_runtime_panel_reconciliation_lock_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_controller_id,
        expected_controller_fencing_token,
        expected_convergence_attempt_no,
        expected_runtime_generation,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision,
        expected_session_id,
        expected_session_record_revision
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    IF locked_row.controller_lease_expires_at
        <= locked_row.checked_at
            + required_lease_headroom_ms * INTERVAL '1 millisecond'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP001',
            MESSAGE = 'runtime_panel_lease_headroom_lost';
    END IF;

    checked_at := locked_row.checked_at;
    controller_lease_expires_at := locked_row.controller_lease_expires_at;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_installation_upsert_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT,
    expected_record_revision BIGINT,
    requested_panel_key TEXT,
    requested_installed_version BIGINT,
    requested_channel_id TEXT,
    requested_message_id TEXT,
    requested_spec_hash TEXT,
    expected_journal_record_revision BIGINT
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    installation_row public.ruleset_panel_installations%ROWTYPE;
    journal_row public.strict_panel_operation_journal%ROWTYPE;
    resident_count BIGINT;
    allocated_revision BIGINT;
BEGIN
    IF expected_record_revision NOT BETWEEN 0 AND 9223372036854775807
        OR pg_catalog.octet_length(requested_panel_key) NOT BETWEEN 1 AND 128
        OR requested_installed_version IS DISTINCT FROM expected_target_version
        OR requested_installed_version NOT BETWEEN 1 AND 4294967295
        OR NOT (CASE
            WHEN requested_channel_id ~ '^[1-9][0-9]{0,19}$'
            THEN requested_channel_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR NOT (CASE
            WHEN requested_message_id ~ '^[1-9][0-9]{0,19}$'
            THEN requested_message_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR requested_spec_hash !~ '^[0-9a-f]{64}$'
        OR expected_journal_record_revision NOT BETWEEN 0 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_installation';
    END IF;

    PERFORM *
    FROM public.starring_runtime_panel_reconciliation_lock_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_controller_id,
        expected_controller_fencing_token,
        expected_convergence_attempt_no,
        expected_runtime_generation,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision,
        expected_session_id,
        expected_session_record_revision
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_persistence_corrupt';
    END IF;

    SELECT *
    INTO installation_row
    FROM public.ruleset_panel_installations AS installation
    WHERE installation.guild_id = expected_guild_id
        AND installation.ruleset_key = expected_ruleset_key
        AND installation.panel_key = requested_panel_key
    FOR UPDATE;

    IF FOUND THEN
        IF expected_record_revision = 0 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_installation_cas_conflict';
        END IF;
        IF installation_row.record_revision IS DISTINCT FROM expected_record_revision THEN
            IF installation_row.installed_version IS NOT DISTINCT FROM requested_installed_version
                AND installation_row.channel_id IS NOT DISTINCT FROM requested_channel_id
                AND installation_row.message_id IS NOT DISTINCT FROM requested_message_id
                AND installation_row.spec_hash IS NOT DISTINCT FROM requested_spec_hash
                AND installation_row.last_reconciliation_session_id
                    IS NOT DISTINCT FROM expected_session_id
                AND installation_row.last_runtime_deployment_id
                    IS NOT DISTINCT FROM expected_deployment_id
                AND installation_row.last_runtime_deployment_revision
                    IS NOT DISTINCT FROM expected_deployment_revision
            THEN
                RETURN installation_row.record_revision;
            END IF;
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_installation_cas_conflict';
        END IF;
        IF installation_row.installed_version IS NOT DISTINCT FROM requested_installed_version
            AND installation_row.channel_id IS NOT DISTINCT FROM requested_channel_id
            AND installation_row.message_id IS NOT DISTINCT FROM requested_message_id
            AND installation_row.spec_hash IS NOT DISTINCT FROM requested_spec_hash
            AND installation_row.last_reconciliation_session_id
                IS NOT DISTINCT FROM expected_session_id
            AND installation_row.last_runtime_deployment_id
                IS NOT DISTINCT FROM expected_deployment_id
            AND installation_row.last_runtime_deployment_revision
                IS NOT DISTINCT FROM expected_deployment_revision
        THEN
            RETURN installation_row.record_revision;
        END IF;
    ELSIF expected_record_revision <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP002',
            MESSAGE = 'runtime_panel_installation_cas_conflict';
    END IF;

    SELECT *
    INTO journal_row
    FROM public.strict_panel_operation_journal AS journal
    WHERE journal.guild_id = expected_guild_id
        AND journal.ruleset_key = expected_ruleset_key
        AND journal.panel_key = requested_panel_key
    FOR UPDATE;

    IF expected_journal_record_revision = 0 THEN
        IF FOUND
            OR installation_row.guild_id IS NULL
            OR installation_row.channel_id IS DISTINCT FROM requested_channel_id
            OR installation_row.message_id IS DISTINCT FROM requested_message_id
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_installation_transition_invalid';
        END IF;
    ELSIF NOT FOUND
        OR journal_row.record_revision IS DISTINCT FROM expected_journal_record_revision
        OR journal_row.state_tag <> 'post_applied'
        OR journal_row.operation_payload #>> '{state,intent,ruleset_version}'
            IS DISTINCT FROM requested_installed_version::TEXT
        OR journal_row.operation_payload #>> '{state,intent,channel_id}'
            IS DISTINCT FROM requested_channel_id
        OR journal_row.operation_payload #>> '{state,intent,spec_hash}'
            IS DISTINCT FROM requested_spec_hash
        OR journal_row.operation_payload #>> '{state,message_id}'
            IS DISTINCT FROM requested_message_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_installation_transition_invalid';
    END IF;

    IF installation_row.guild_id IS NULL
        AND journal_row.guild_id IS NULL
    THEN
        SELECT pg_catalog.count(*)
        INTO resident_count
        FROM (
            SELECT installation.panel_key
            FROM public.ruleset_panel_installations AS installation
            WHERE installation.guild_id = expected_guild_id
                AND installation.ruleset_key = expected_ruleset_key
            UNION
            SELECT journal.panel_key
            FROM public.strict_panel_operation_journal AS journal
            WHERE journal.guild_id = expected_guild_id
                AND journal.ruleset_key = expected_ruleset_key
        ) AS resident;
        IF resident_count > 256 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_persistence_corrupt';
        END IF;
        IF resident_count = 256 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP003',
                MESSAGE = 'runtime_panel_capacity_exceeded';
        END IF;
    END IF;

    UPDATE public.runtime_panel_reconciliation_sessions AS session
    SET next_record_revision = session.next_record_revision + 1,
        updated_at = pg_catalog.clock_timestamp()
    WHERE session.guild_id = expected_guild_id
        AND session.ruleset_key = expected_ruleset_key
        AND session.session_id = expected_session_id
        AND session.session_record_revision = expected_session_record_revision
        AND session.next_record_revision BETWEEN 2 AND 9223372036854775806
    RETURNING session.next_record_revision - 1
    INTO allocated_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_revision_exhausted';
    END IF;

    INSERT INTO public.ruleset_panel_installations (
        guild_id,
        ruleset_key,
        panel_key,
        installed_version,
        channel_id,
        message_id,
        spec_hash,
        record_revision,
        last_reconciliation_session_id,
        last_runtime_deployment_id,
        last_runtime_deployment_revision
    ) VALUES (
        expected_guild_id,
        expected_ruleset_key,
        requested_panel_key,
        requested_installed_version,
        requested_channel_id,
        requested_message_id,
        requested_spec_hash,
        allocated_revision,
        expected_session_id,
        expected_deployment_id,
        expected_deployment_revision
    )
    ON CONFLICT (guild_id, ruleset_key, panel_key) DO UPDATE
    SET installed_version = EXCLUDED.installed_version,
        channel_id = EXCLUDED.channel_id,
        message_id = EXCLUDED.message_id,
        spec_hash = EXCLUDED.spec_hash,
        record_revision = EXCLUDED.record_revision,
        last_reconciliation_session_id = EXCLUDED.last_reconciliation_session_id,
        last_runtime_deployment_id = EXCLUDED.last_runtime_deployment_id,
        last_runtime_deployment_revision = EXCLUDED.last_runtime_deployment_revision;

    RETURN allocated_revision;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_installation_remove_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT,
    expected_record_revision BIGINT,
    requested_panel_key TEXT
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    installation_row public.ruleset_panel_installations%ROWTYPE;
    journal_row public.strict_panel_operation_journal%ROWTYPE;
    allocated_revision BIGINT;
BEGIN
    IF expected_record_revision NOT BETWEEN 0 AND 9223372036854775807
        OR pg_catalog.octet_length(requested_panel_key) NOT BETWEEN 1 AND 128
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_installation';
    END IF;

    PERFORM *
    FROM public.starring_runtime_panel_reconciliation_lock_v1(
        expected_tenant_id, expected_installation_id, expected_deployment_id,
        expected_deployment_revision, expected_controller_id,
        expected_controller_fencing_token, expected_convergence_attempt_no,
        expected_runtime_generation, expected_guild_id, expected_ruleset_key,
        expected_target_version, expected_target_content_hash,
        expected_binding_revision, expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision, expected_session_id,
        expected_session_record_revision
    );

    SELECT *
    INTO installation_row
    FROM public.ruleset_panel_installations AS installation
    WHERE installation.guild_id = expected_guild_id
        AND installation.ruleset_key = expected_ruleset_key
        AND installation.panel_key = requested_panel_key
    FOR UPDATE;

    IF NOT FOUND THEN
        SELECT session.next_record_revision - 1
        INTO allocated_revision
        FROM public.runtime_panel_reconciliation_sessions AS session
        WHERE session.guild_id = expected_guild_id
            AND session.ruleset_key = expected_ruleset_key;
        IF allocated_revision IS NULL
            OR allocated_revision < 1
            OR (
                expected_record_revision > 0
                AND allocated_revision <= expected_record_revision
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_installation_cas_conflict';
        END IF;
        RETURN allocated_revision;
    END IF;

    IF installation_row.record_revision IS DISTINCT FROM expected_record_revision THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP002',
            MESSAGE = 'runtime_panel_installation_cas_conflict';
    END IF;

    SELECT *
    INTO journal_row
    FROM public.strict_panel_operation_journal AS journal
    WHERE journal.guild_id = expected_guild_id
        AND journal.ruleset_key = expected_ruleset_key
        AND journal.panel_key = requested_panel_key
    FOR UPDATE;

    IF NOT FOUND
        OR journal_row.state_tag <> 'cleanup_pending'
        OR journal_row.operation_payload #>> '{state,intent,kind}' <> 'removed'
        OR journal_row.operation_payload #>> '{state,intent,remove_installation}' <> 'true'
        OR journal_row.operation_payload #>> '{state,intent,message,channel_id}'
            IS DISTINCT FROM installation_row.channel_id
        OR journal_row.operation_payload #>> '{state,intent,message,message_id}'
            IS DISTINCT FROM installation_row.message_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_installation_transition_invalid';
    END IF;

    UPDATE public.runtime_panel_reconciliation_sessions AS session
    SET next_record_revision = session.next_record_revision + 1,
        updated_at = pg_catalog.clock_timestamp()
    WHERE session.guild_id = expected_guild_id
        AND session.ruleset_key = expected_ruleset_key
        AND session.session_id = expected_session_id
        AND session.session_record_revision = expected_session_record_revision
        AND session.next_record_revision BETWEEN 2 AND 9223372036854775806
    RETURNING session.next_record_revision - 1
    INTO allocated_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_revision_exhausted';
    END IF;

    DELETE FROM public.ruleset_panel_installations AS installation
    WHERE installation.guild_id = expected_guild_id
        AND installation.ruleset_key = expected_ruleset_key
        AND installation.panel_key = requested_panel_key
        AND installation.record_revision = expected_record_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP002',
            MESSAGE = 'runtime_panel_installation_cas_conflict';
    END IF;

    RETURN allocated_revision;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_journal_put_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT,
    expected_record_revision BIGINT,
    requested_record_format_version SMALLINT,
    requested_panel_key TEXT,
    requested_state_tag TEXT,
    requested_operation_payload JSONB
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    journal_row public.strict_panel_operation_journal%ROWTYPE;
    installation_row public.ruleset_panel_installations%ROWTYPE;
    installation_exists BOOLEAN;
    resident_count BIGINT;
    allocated_revision BIGINT;
    valid_transition BOOLEAN;
BEGIN
    IF expected_record_revision NOT BETWEEN 0 AND 9223372036854775807
        OR requested_record_format_version <> 1
        OR pg_catalog.octet_length(requested_panel_key) NOT BETWEEN 1 AND 128
        OR requested_state_tag NOT IN (
            'post_dispatching',
            'post_applied',
            'ambiguous_post',
            'cleanup_pending'
        )
        OR pg_catalog.octet_length(requested_operation_payload::TEXT)
            NOT BETWEEN 32 AND 245760
        OR (CASE
            WHEN pg_catalog.jsonb_typeof(requested_operation_payload) = 'object'
                AND pg_catalog.jsonb_typeof(requested_operation_payload -> 'key') = 'object'
                AND pg_catalog.jsonb_typeof(requested_operation_payload -> 'state') = 'object'
            THEN requested_operation_payload = pg_catalog.jsonb_build_object(
                    'key', requested_operation_payload -> 'key',
                    'state', requested_operation_payload -> 'state'
                )
                AND requested_operation_payload -> 'key' = pg_catalog.jsonb_build_object(
                    'guild_id', requested_operation_payload #> '{key,guild_id}',
                    'ruleset_key', requested_operation_payload #> '{key,ruleset_key}',
                    'panel_key', requested_operation_payload #> '{key,panel_key}'
                )
                AND requested_operation_payload #>> '{key,guild_id}' = expected_guild_id
                AND requested_operation_payload #>> '{key,ruleset_key}' = expected_ruleset_key
                AND requested_operation_payload #>> '{key,panel_key}' = requested_panel_key
                AND requested_operation_payload #>> '{state,state}' = requested_state_tag
            ELSE FALSE
        END) IS NOT TRUE
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    IF requested_state_tag IN ('post_dispatching', 'post_applied', 'ambiguous_post')
        AND (
            (CASE
                WHEN pg_catalog.jsonb_typeof(
                    requested_operation_payload #> '{state,intent}'
                ) = 'object'
                THEN requested_operation_payload -> 'state' = CASE
                        WHEN requested_state_tag = 'post_applied'
                        THEN pg_catalog.jsonb_build_object(
                            'state', requested_operation_payload #> '{state,state}',
                            'intent', requested_operation_payload #> '{state,intent}',
                            'message_id', requested_operation_payload #> '{state,message_id}'
                        )
                        ELSE pg_catalog.jsonb_build_object(
                            'state', requested_operation_payload #> '{state,state}',
                            'intent', requested_operation_payload #> '{state,intent}'
                        )
                    END
                    AND requested_operation_payload #> '{state,intent}'
                        = pg_catalog.jsonb_build_object(
                            'panel', requested_operation_payload #> '{state,intent,panel}',
                            'ruleset_version', requested_operation_payload
                                #> '{state,intent,ruleset_version}',
                            'channel_id', requested_operation_payload
                                #> '{state,intent,channel_id}',
                            'spec_hash', requested_operation_payload
                                #> '{state,intent,spec_hash}',
                            'install_kind', requested_operation_payload
                                #> '{state,intent,install_kind}',
                            'previous_message', requested_operation_payload
                                #> '{state,intent,previous_message}'
                        )
                ELSE FALSE
            END) IS NOT TRUE
            OR requested_operation_payload #>> '{state,intent,panel,spec,key}'
                IS DISTINCT FROM requested_panel_key
            OR requested_operation_payload #>> '{state,intent,ruleset_version}'
                IS DISTINCT FROM expected_target_version::TEXT
            OR requested_operation_payload #>> '{state,intent,spec_hash}'
                !~ '^[0-9a-f]{64}$'
            OR NOT (CASE
                WHEN requested_operation_payload #>> '{state,intent,channel_id}'
                    ~ '^[1-9][0-9]{0,19}$'
                THEN (requested_operation_payload #>> '{state,intent,channel_id}')::NUMERIC
                    <= 18446744073709551615
                ELSE FALSE
            END)
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    IF requested_state_tag IN ('post_dispatching', 'post_applied', 'ambiguous_post')
        AND NOT (
            (
                requested_operation_payload #>> '{state,intent,install_kind}'
                    IN ('fresh', 'missing_message')
                AND requested_operation_payload #> '{state,intent,previous_message}'
                    IS NOT DISTINCT FROM 'null'::JSONB
            )
            OR (
                requested_operation_payload #>> '{state,intent,install_kind}'
                    = 'channel_moved'
                AND requested_operation_payload
                    #>> '{state,intent,previous_message,cleanup_kind}'
                    = 'channel_moved'
            )
            OR (
                requested_operation_payload #>> '{state,intent,install_kind}'
                    = 'payload_replaced'
                AND requested_operation_payload
                    #>> '{state,intent,previous_message,cleanup_kind}'
                    = 'payload_replaced'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    IF requested_state_tag IN ('post_dispatching', 'post_applied', 'ambiguous_post')
        AND requested_operation_payload #> '{state,intent,previous_message}'
            IS DISTINCT FROM 'null'::JSONB
        AND (
            (CASE
                WHEN pg_catalog.jsonb_typeof(
                    requested_operation_payload #> '{state,intent,previous_message}'
                ) = 'object'
                    AND pg_catalog.jsonb_typeof(
                        requested_operation_payload
                            #> '{state,intent,previous_message,message}'
                    ) = 'object'
                THEN requested_operation_payload #> '{state,intent,previous_message}'
                        = pg_catalog.jsonb_build_object(
                            'message', requested_operation_payload
                                #> '{state,intent,previous_message,message}',
                            'cleanup_kind', requested_operation_payload
                                #> '{state,intent,previous_message,cleanup_kind}'
                        )
                    AND requested_operation_payload
                            #> '{state,intent,previous_message,message}'
                        = pg_catalog.jsonb_build_object(
                            'channel_id', requested_operation_payload
                                #> '{state,intent,previous_message,message,channel_id}',
                            'message_id', requested_operation_payload
                                #> '{state,intent,previous_message,message,message_id}'
                        )
                ELSE FALSE
            END) IS NOT TRUE
            OR NOT (CASE
                WHEN requested_operation_payload
                    #>> '{state,intent,previous_message,message,channel_id}'
                    ~ '^[1-9][0-9]{0,19}$'
                THEN (requested_operation_payload
                    #>> '{state,intent,previous_message,message,channel_id}')::NUMERIC
                    <= 18446744073709551615
                ELSE FALSE
            END)
            OR NOT (CASE
                WHEN requested_operation_payload
                    #>> '{state,intent,previous_message,message,message_id}'
                    ~ '^[1-9][0-9]{0,19}$'
                THEN (requested_operation_payload
                    #>> '{state,intent,previous_message,message,message_id}')::NUMERIC
                    <= 18446744073709551615
                ELSE FALSE
            END)
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    IF requested_state_tag = 'post_applied'
        AND NOT (CASE
            WHEN requested_operation_payload #>> '{state,message_id}'
                ~ '^[1-9][0-9]{0,19}$'
            THEN (requested_operation_payload #>> '{state,message_id}')::NUMERIC
                <= 18446744073709551615
            ELSE FALSE
        END)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    IF requested_state_tag = 'post_applied'
        AND requested_operation_payload #> '{state,intent,previous_message}'
            IS DISTINCT FROM 'null'::JSONB
        AND requested_operation_payload
            #>> '{state,intent,previous_message,message,channel_id}'
            IS NOT DISTINCT FROM requested_operation_payload #>> '{state,intent,channel_id}'
        AND requested_operation_payload
            #>> '{state,intent,previous_message,message,message_id}'
            IS NOT DISTINCT FROM requested_operation_payload #>> '{state,message_id}'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    IF requested_state_tag = 'cleanup_pending'
        AND (
            (CASE
                WHEN pg_catalog.jsonb_typeof(
                    requested_operation_payload #> '{state,intent}'
                ) = 'object'
                    AND pg_catalog.jsonb_typeof(
                        requested_operation_payload #> '{state,intent,message}'
                    ) = 'object'
                THEN requested_operation_payload -> 'state'
                        = pg_catalog.jsonb_build_object(
                            'state', requested_operation_payload #> '{state,state}',
                            'intent', requested_operation_payload #> '{state,intent}'
                        )
                    AND requested_operation_payload #> '{state,intent}'
                        = pg_catalog.jsonb_build_object(
                            'message', requested_operation_payload
                                #> '{state,intent,message}',
                            'kind', requested_operation_payload #> '{state,intent,kind}',
                            'remove_installation', requested_operation_payload
                                #> '{state,intent,remove_installation}'
                        )
                    AND requested_operation_payload #> '{state,intent,message}'
                        = pg_catalog.jsonb_build_object(
                            'channel_id', requested_operation_payload
                                #> '{state,intent,message,channel_id}',
                            'message_id', requested_operation_payload
                                #> '{state,intent,message,message_id}'
                        )
                ELSE FALSE
            END) IS NOT TRUE
            OR requested_operation_payload #>> '{state,intent,remove_installation}'
                NOT IN ('true', 'false')
            OR requested_operation_payload #>> '{state,intent,kind}'
                NOT IN ('removed', 'channel_moved', 'payload_replaced', 'orphan')
            OR (
                requested_operation_payload #>> '{state,intent,kind}' = 'removed'
            ) IS DISTINCT FROM (
                requested_operation_payload #>> '{state,intent,remove_installation}' = 'true'
            )
            OR NOT (CASE
                WHEN requested_operation_payload #>> '{state,intent,message,channel_id}'
                    ~ '^[1-9][0-9]{0,19}$'
                THEN (requested_operation_payload
                    #>> '{state,intent,message,channel_id}')::NUMERIC
                    <= 18446744073709551615
                ELSE FALSE
            END)
            OR NOT (CASE
                WHEN requested_operation_payload #>> '{state,intent,message,message_id}'
                    ~ '^[1-9][0-9]{0,19}$'
                THEN (requested_operation_payload
                    #>> '{state,intent,message,message_id}')::NUMERIC
                    <= 18446744073709551615
                ELSE FALSE
            END)
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    PERFORM *
    FROM public.starring_runtime_panel_reconciliation_lock_v1(
        expected_tenant_id, expected_installation_id, expected_deployment_id,
        expected_deployment_revision, expected_controller_id,
        expected_controller_fencing_token, expected_convergence_attempt_no,
        expected_runtime_generation, expected_guild_id, expected_ruleset_key,
        expected_target_version, expected_target_content_hash,
        expected_binding_revision, expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision, expected_session_id,
        expected_session_record_revision
    );

    SELECT *
    INTO journal_row
    FROM public.strict_panel_operation_journal AS journal
    WHERE journal.guild_id = expected_guild_id
        AND journal.ruleset_key = expected_ruleset_key
        AND journal.panel_key = requested_panel_key
    FOR UPDATE;

    IF FOUND THEN
        IF journal_row.state_tag = 'post_dispatching'
            AND requested_state_tag = 'post_dispatching'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_post_dispatch_conflict';
        END IF;
        IF expected_record_revision = 0 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_journal_cas_conflict';
        END IF;
        IF journal_row.record_revision IS DISTINCT FROM expected_record_revision THEN
            IF journal_row.record_format_version
                    IS NOT DISTINCT FROM requested_record_format_version
                AND journal_row.state_tag IS NOT DISTINCT FROM requested_state_tag
                AND journal_row.operation_payload IS NOT DISTINCT FROM requested_operation_payload
                AND journal_row.last_reconciliation_session_id
                    IS NOT DISTINCT FROM expected_session_id
                AND journal_row.last_runtime_deployment_id
                    IS NOT DISTINCT FROM expected_deployment_id
                AND journal_row.last_runtime_deployment_revision
                    IS NOT DISTINCT FROM expected_deployment_revision
            THEN
                RETURN journal_row.record_revision;
            END IF;
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_journal_cas_conflict';
        END IF;
        IF journal_row.record_format_version
                IS NOT DISTINCT FROM requested_record_format_version
            AND journal_row.state_tag IS NOT DISTINCT FROM requested_state_tag
            AND journal_row.operation_payload IS NOT DISTINCT FROM requested_operation_payload
            AND journal_row.last_reconciliation_session_id
                IS NOT DISTINCT FROM expected_session_id
            AND journal_row.last_runtime_deployment_id
                IS NOT DISTINCT FROM expected_deployment_id
            AND journal_row.last_runtime_deployment_revision
                IS NOT DISTINCT FROM expected_deployment_revision
        THEN
            RETURN journal_row.record_revision;
        END IF;

        valid_transition := (
            journal_row.state_tag = 'post_dispatching'
            AND requested_state_tag IN ('post_applied', 'ambiguous_post')
            AND journal_row.operation_payload #> '{state,intent}'
                IS NOT DISTINCT FROM requested_operation_payload #> '{state,intent}'
        ) OR (
            journal_row.state_tag = 'post_applied'
            AND requested_state_tag = 'cleanup_pending'
            AND requested_operation_payload #>> '{state,intent,kind}' = 'orphan'
            AND requested_operation_payload #>> '{state,intent,remove_installation}' = 'false'
            AND requested_operation_payload #>> '{state,intent,message,channel_id}'
                IS NOT DISTINCT FROM journal_row.operation_payload
                    #>> '{state,intent,channel_id}'
            AND requested_operation_payload #>> '{state,intent,message,message_id}'
                IS NOT DISTINCT FROM journal_row.operation_payload #>> '{state,message_id}'
        );
        IF valid_transition IS NOT TRUE THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_journal_transition_invalid';
        END IF;
    ELSE
        IF expected_record_revision <> 0 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_journal_cas_conflict';
        END IF;
        IF requested_state_tag NOT IN ('post_dispatching', 'cleanup_pending') THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_journal_transition_invalid';
        END IF;
        IF requested_state_tag = 'cleanup_pending'
            AND (
                requested_operation_payload #>> '{state,intent,kind}' <> 'removed'
                OR requested_operation_payload
                    #>> '{state,intent,remove_installation}' <> 'true'
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_journal_transition_invalid';
        END IF;
    END IF;

    SELECT *
    INTO installation_row
    FROM public.ruleset_panel_installations AS installation
    WHERE installation.guild_id = expected_guild_id
        AND installation.ruleset_key = expected_ruleset_key
        AND installation.panel_key = requested_panel_key
    FOR UPDATE;
    installation_exists := FOUND;

    IF journal_row.guild_id IS NULL
        AND requested_state_tag = 'post_dispatching'
        AND NOT (
            (
                requested_operation_payload #>> '{state,intent,install_kind}' = 'fresh'
                AND NOT installation_exists
            )
            OR (
                requested_operation_payload #>> '{state,intent,install_kind}'
                    = 'missing_message'
                AND installation_exists
                AND installation_row.channel_id IS NOT DISTINCT FROM
                    requested_operation_payload #>> '{state,intent,channel_id}'
            )
            OR (
                requested_operation_payload #>> '{state,intent,install_kind}'
                    IN ('channel_moved', 'payload_replaced')
                AND installation_exists
                AND installation_row.channel_id IS NOT DISTINCT FROM
                    requested_operation_payload
                        #>> '{state,intent,previous_message,message,channel_id}'
                AND installation_row.message_id IS NOT DISTINCT FROM
                    requested_operation_payload
                        #>> '{state,intent,previous_message,message,message_id}'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_journal_transition_invalid';
    END IF;

    IF journal_row.guild_id IS NULL
        AND requested_state_tag = 'cleanup_pending'
        AND (
            NOT installation_exists
            OR requested_operation_payload #>> '{state,intent,message,channel_id}'
                IS DISTINCT FROM installation_row.channel_id
            OR requested_operation_payload #>> '{state,intent,message,message_id}'
                IS DISTINCT FROM installation_row.message_id
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_journal_transition_invalid';
    END IF;

    IF journal_row.guild_id IS NULL AND NOT installation_exists THEN
        SELECT pg_catalog.count(*)
        INTO resident_count
        FROM (
            SELECT installation.panel_key
            FROM public.ruleset_panel_installations AS installation
            WHERE installation.guild_id = expected_guild_id
                AND installation.ruleset_key = expected_ruleset_key
            UNION
            SELECT journal.panel_key
            FROM public.strict_panel_operation_journal AS journal
            WHERE journal.guild_id = expected_guild_id
                AND journal.ruleset_key = expected_ruleset_key
        ) AS resident;
        IF resident_count > 256 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP004',
                MESSAGE = 'runtime_panel_persistence_corrupt';
        END IF;
        IF resident_count = 256 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP003',
                MESSAGE = 'runtime_panel_capacity_exceeded';
        END IF;
    END IF;

    UPDATE public.runtime_panel_reconciliation_sessions AS session
    SET next_record_revision = session.next_record_revision + 1,
        updated_at = pg_catalog.clock_timestamp()
    WHERE session.guild_id = expected_guild_id
        AND session.ruleset_key = expected_ruleset_key
        AND session.session_id = expected_session_id
        AND session.session_record_revision = expected_session_record_revision
        AND session.next_record_revision BETWEEN 2 AND 9223372036854775806
    RETURNING session.next_record_revision - 1
    INTO allocated_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_revision_exhausted';
    END IF;

    INSERT INTO public.strict_panel_operation_journal (
        record_format_version,
        guild_id,
        ruleset_key,
        panel_key,
        state_tag,
        operation_payload,
        record_revision,
        last_reconciliation_session_id,
        last_runtime_deployment_id,
        last_runtime_deployment_revision,
        updated_at
    ) VALUES (
        requested_record_format_version,
        expected_guild_id,
        expected_ruleset_key,
        requested_panel_key,
        requested_state_tag,
        requested_operation_payload,
        allocated_revision,
        expected_session_id,
        expected_deployment_id,
        expected_deployment_revision,
        pg_catalog.clock_timestamp()
    )
    ON CONFLICT (guild_id, ruleset_key, panel_key) DO UPDATE
    SET record_format_version = EXCLUDED.record_format_version,
        state_tag = EXCLUDED.state_tag,
        operation_payload = EXCLUDED.operation_payload,
        record_revision = EXCLUDED.record_revision,
        last_reconciliation_session_id = EXCLUDED.last_reconciliation_session_id,
        last_runtime_deployment_id = EXCLUDED.last_runtime_deployment_id,
        last_runtime_deployment_revision = EXCLUDED.last_runtime_deployment_revision,
        updated_at = EXCLUDED.updated_at;

    RETURN allocated_revision;
END;
$function$;

CREATE FUNCTION public.starring_runtime_panel_reconciliation_journal_remove_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_current_authority_revision BIGINT,
    expected_session_id TEXT,
    expected_session_record_revision BIGINT,
    expected_record_revision BIGINT,
    requested_panel_key TEXT
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    journal_row public.strict_panel_operation_journal%ROWTYPE;
    allocated_revision BIGINT;
BEGIN
    IF expected_record_revision NOT BETWEEN 0 AND 9223372036854775807
        OR pg_catalog.octet_length(requested_panel_key) NOT BETWEEN 1 AND 128
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_invalid_journal';
    END IF;

    PERFORM *
    FROM public.starring_runtime_panel_reconciliation_lock_v1(
        expected_tenant_id, expected_installation_id, expected_deployment_id,
        expected_deployment_revision, expected_controller_id,
        expected_controller_fencing_token, expected_convergence_attempt_no,
        expected_runtime_generation, expected_guild_id, expected_ruleset_key,
        expected_target_version, expected_target_content_hash,
        expected_binding_revision, expected_binding_fingerprint,
        expected_installation_authority_revision,
        expected_current_authority_revision, expected_session_id,
        expected_session_record_revision
    );

    SELECT *
    INTO journal_row
    FROM public.strict_panel_operation_journal AS journal
    WHERE journal.guild_id = expected_guild_id
        AND journal.ruleset_key = expected_ruleset_key
        AND journal.panel_key = requested_panel_key
    FOR UPDATE;

    IF NOT FOUND THEN
        SELECT session.next_record_revision - 1
        INTO allocated_revision
        FROM public.runtime_panel_reconciliation_sessions AS session
        WHERE session.guild_id = expected_guild_id
            AND session.ruleset_key = expected_ruleset_key;
        IF allocated_revision IS NULL
            OR allocated_revision < 1
            OR (
                expected_record_revision > 0
                AND allocated_revision <= expected_record_revision
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RP002',
                MESSAGE = 'runtime_panel_journal_cas_conflict';
        END IF;
        RETURN allocated_revision;
    END IF;

    IF journal_row.record_revision IS DISTINCT FROM expected_record_revision THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP002',
            MESSAGE = 'runtime_panel_journal_cas_conflict';
    END IF;

    IF journal_row.state_tag = 'ambiguous_post' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_journal_transition_invalid';
    END IF;

    UPDATE public.runtime_panel_reconciliation_sessions AS session
    SET next_record_revision = session.next_record_revision + 1,
        updated_at = pg_catalog.clock_timestamp()
    WHERE session.guild_id = expected_guild_id
        AND session.ruleset_key = expected_ruleset_key
        AND session.session_id = expected_session_id
        AND session.session_record_revision = expected_session_record_revision
        AND session.next_record_revision BETWEEN 2 AND 9223372036854775806
    RETURNING session.next_record_revision - 1
    INTO allocated_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_revision_exhausted';
    END IF;

    DELETE FROM public.strict_panel_operation_journal AS journal
    WHERE journal.guild_id = expected_guild_id
        AND journal.ruleset_key = expected_ruleset_key
        AND journal.panel_key = requested_panel_key
        AND journal.record_revision = expected_record_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP002',
            MESSAGE = 'runtime_panel_journal_cas_conflict';
    END IF;

    RETURN allocated_revision;
END;
$function$;

DO $restrict_acl$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    function_oid OID;
    relation_identity TEXT;
    relation_oid OID;
    grantee OID;
    grantee_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_panel_reconciliation_sessions'
    );
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'runtime panel owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOR relation_identity IN
        SELECT identity
        FROM (
            VALUES
                ('public.runtime_panel_reconciliation_sessions'),
                ('public.ruleset_panel_installations'),
                ('public.strict_panel_operation_journal')
        ) AS expected(identity)
    LOOP
        relation_oid := pg_catalog.to_regclass(relation_identity);
        IF relation_oid IS NULL THEN
            RAISE EXCEPTION 'runtime panel relation is unavailable'
                USING ERRCODE = '55000';
        END IF;
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
            WHERE relation.oid = relation_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime panel relation grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON TABLE %s FROM %I CASCADE',
                relation_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    FOR function_identity IN
        SELECT identity
        FROM (
            VALUES
                ('public.starring_runtime_panel_execution_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint)'),
                ('public.starring_runtime_panel_reconciliation_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)'),
                ('public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)'),
                ('public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)'),
                ('public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)'),
                ('public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)'),
                ('public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)'),
                ('public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)'),
                ('public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)')
        ) AS expected(identity)
    LOOP
        function_oid := pg_catalog.to_regprocedure(function_identity);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'runtime panel function is unavailable'
                USING ERRCODE = '55000';
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
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime panel function grantee is unavailable'
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
$restrict_acl$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_column_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_function_count BIGINT;
    overload_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_panel_reconciliation_sessions'
    );
    IF common_owner IS NULL THEN
        RAISE EXCEPTION 'runtime panel owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_panel_reconciliation_sessions'),
            ('public.ruleset_panel_installations'),
            ('public.strict_panel_operation_journal')
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
        );

    IF invalid_relation_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel relation contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_column_count
    FROM (
        VALUES
            ('public.ruleset_panel_installations', 'record_revision', 'bigint', TRUE),
            ('public.ruleset_panel_installations', 'last_reconciliation_session_id', 'text', FALSE),
            ('public.ruleset_panel_installations', 'last_runtime_deployment_id', 'text', FALSE),
            ('public.ruleset_panel_installations', 'last_runtime_deployment_revision', 'bigint', FALSE),
            ('public.strict_panel_operation_journal', 'record_revision', 'bigint', TRUE),
            ('public.strict_panel_operation_journal', 'last_reconciliation_session_id', 'text', FALSE),
            ('public.strict_panel_operation_journal', 'last_runtime_deployment_id', 'text', FALSE),
            ('public.strict_panel_operation_journal', 'last_runtime_deployment_revision', 'bigint', FALSE),
            ('public.runtime_panel_reconciliation_sessions', 'session_record_revision', 'bigint', TRUE),
            ('public.runtime_panel_reconciliation_sessions', 'next_record_revision', 'bigint', TRUE),
            ('public.runtime_panel_reconciliation_sessions', 'controller_lease_expires_at', 'timestamp with time zone', TRUE)
    ) AS expected(relation_identity, column_name, type_name, not_null)
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND attribute.attname = expected.column_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attnum IS NULL
        OR pg_catalog.format_type(attribute.atttypid, attribute.atttypmod)
            IS DISTINCT FROM expected.type_name
        OR attribute.attnotnull IS DISTINCT FROM expected.not_null;

    IF invalid_column_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel column contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            ('public.ruleset_panel_installations', 'ruleset_panel_installations_runtime_provenance_valid'),
            ('public.strict_panel_operation_journal', 'strict_panel_operation_journal_runtime_provenance_valid'),
            ('public.runtime_panel_reconciliation_sessions', 'runtime_panel_reconciliation_sessions_deployment_fk'),
            ('public.runtime_panel_reconciliation_sessions', 'runtime_panel_reconciliation_sessions_slot_valid'),
            ('public.runtime_panel_reconciliation_sessions', 'runtime_panel_reconciliation_sessions_scope_valid'),
            ('public.runtime_panel_reconciliation_sessions', 'runtime_panel_reconciliation_sessions_target_valid'),
            ('public.runtime_panel_reconciliation_sessions', 'runtime_panel_reconciliation_sessions_identity_valid'),
            ('public.runtime_panel_reconciliation_sessions', 'runtime_panel_reconciliation_sessions_time_valid')
    ) AS expected(relation_identity, constraint_name)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND constraint_row.conname = expected.constraint_name
    WHERE constraint_row.oid IS NULL
        OR NOT constraint_row.convalidated;

    IF invalid_constraint_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel constraint contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO overload_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_panel_execution_lock_v1',
            'starring_runtime_panel_reconciliation_lock_v1',
            'starring_runtime_panel_reconciliation_claim_v1',
            'starring_runtime_panel_reconciliation_check_v1',
            'starring_runtime_panel_reconciliation_snapshot_v1',
            'starring_runtime_panel_reconciliation_installation_upsert_v1',
            'starring_runtime_panel_reconciliation_installation_remove_v1',
            'starring_runtime_panel_reconciliation_journal_put_v1',
            'starring_runtime_panel_reconciliation_journal_remove_v1'
        );

    IF overload_count <> 9 THEN
        RAISE EXCEPTION 'runtime panel function set is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_panel_execution_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint)', 'TABLE(checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)', 'TABLE(checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)', 'TABLE(session_record_revision bigint, checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)', 'TABLE(checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)', 'TABLE(record_kind text, record_revision bigint, record_format_version smallint, guild_id text, ruleset_key text, panel_key text, installed_version bigint, channel_id text, message_id text, spec_hash text, state_tag text, operation_payload jsonb)', TRUE, 512::REAL),
            ('public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)', 'bigint', FALSE, 0::REAL),
            ('public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)', 'bigint', FALSE, 0::REAL),
            ('public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)', 'bigint', FALSE, 0::REAL),
            ('public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)', 'bigint', FALSE, 0::REAL)
    ) AS expected(identity, result_name, returns_set, rows_estimate)
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
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_name
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel function contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
