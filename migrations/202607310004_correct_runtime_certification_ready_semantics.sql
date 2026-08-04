SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-certification-ready-semantics-v1',
        0
    )
);

DO $runtime_certification_ready_semantics$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_certification_commit_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,bytea,text,bytea,text)';
    function_oid OID;
    function_definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    function_oid := pg_catalog.to_regprocedure(function_identity);
    IF function_oid IS NULL THEN
        RAISE EXCEPTION
            'runtime certification ready semantics function precondition failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.jsonb_build_object(
        'oid', function_row.oid::TEXT,
        'owner', function_row.proowner::TEXT,
        'acl', pg_catalog.to_jsonb(function_row.proacl),
        'language', function_row.prolang::TEXT,
        'kind', function_row.prokind,
        'volatile', function_row.provolatile,
        'strict', function_row.proisstrict,
        'security_definer', function_row.prosecdef,
        'parallel', function_row.proparallel,
        'returns_set', function_row.proretset,
        'rows', function_row.prorows,
        'config', pg_catalog.to_jsonb(function_row.proconfig),
        'leakproof', function_row.proleakproof,
        'argument_defaults', function_row.pronargdefaults,
        'variadic', function_row.provariadic::TEXT,
        'return_type', function_row.prorettype::TEXT
    )
    INTO metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    function_definition := pg_catalog.pg_get_functiondef(function_oid);

    previous_fragment :=
        '    next_snapshot JSONB;' || E'\n' ||
        '    live_value JSONB;' || E'\n' ||
        '    database_now TIMESTAMPTZ;';
    next_fragment :=
        '    next_snapshot JSONB;' || E'\n' ||
        '    live_value JSONB;' || E'\n' ||
        '    gateway_ready_value JSONB;' || E'\n' ||
        '    gateway_ready_kind TEXT;' || E'\n' ||
        '    database_now TIMESTAMPTZ;';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready declaration replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        OR route_record #>> ''{gateway,kind}'' IS DISTINCT FROM ''resumed''';
    next_fragment :=
        '        OR COALESCE(' || E'\n' ||
        '            route_record #>> ''{gateway,kind}'',' || E'\n' ||
        '            ''''' || E'\n' ||
        '        ) NOT IN (''ready'', ''resumed'')';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready kind replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        OR deployment_row.snapshot -> ''gateway_ready'' IS NULL' || E'\n' ||
        '        OR deployment_row.snapshot -> ''gateway_ready'' = ''null''::JSONB';
    next_fragment :=
        '        OR deployment_row.snapshot -> ''gateway_ready''' || E'\n' ||
        '            IS DISTINCT FROM ''null''::JSONB';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready prestate replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    live_value := pg_catalog.jsonb_build_object(';
    next_fragment :=
        '    gateway_ready_kind := CASE route_record #>> ''{gateway,kind}''' || E'\n' ||
        '        WHEN ''ready'' THEN ''discord_ready''' || E'\n' ||
        '        WHEN ''resumed'' THEN ''discord_resumed''' || E'\n' ||
        '    END;' || E'\n' ||
        '    gateway_ready_value := pg_catalog.jsonb_build_object(' || E'\n' ||
        '        ''target'', intent -> ''target'',' || E'\n' ||
        '        ''runtime_generation'', expected_runtime_generation,' || E'\n' ||
        '        ''process_instance_id'',' || E'\n' ||
        '            route_record #>> ''{gateway,process_instance_id}'',' || E'\n' ||
        '        ''kind'', gateway_ready_kind,' || E'\n' ||
        '        ''ready_at'', pg_catalog.to_jsonb(database_now)' || E'\n' ||
        '    );' || E'\n' ||
        '    live_value := pg_catalog.jsonb_build_object(';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready projection replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        ''gateway_ready'', deployment_row.snapshot -> ''gateway_ready'',';
    next_fragment :=
        '        ''gateway_ready'', gateway_ready_value,';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready live projection replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    next_snapshot := pg_catalog.jsonb_set(' || E'\n' ||
        '        next_snapshot,' || E'\n' ||
        '        ''{live}'',' || E'\n' ||
        '        live_value,' || E'\n' ||
        '        FALSE' || E'\n' ||
        '    );';
    next_fragment :=
        '    next_snapshot := pg_catalog.jsonb_set(' || E'\n' ||
        '        next_snapshot,' || E'\n' ||
        '        ''{gateway_ready}'',' || E'\n' ||
        '        gateway_ready_value,' || E'\n' ||
        '        FALSE' || E'\n' ||
        '    );' || E'\n' ||
        '    next_snapshot := pg_catalog.jsonb_set(' || E'\n' ||
        '        next_snapshot,' || E'\n' ||
        '        ''{live}'',' || E'\n' ||
        '        live_value,' || E'\n' ||
        '        FALSE' || E'\n' ||
        '    );';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready snapshot replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        ''discord_resumed'',' || E'\n' ||
        '        database_now,' || E'\n' ||
        '        database_now,' || E'\n' ||
        '        2,';
    next_fragment :=
        '        gateway_ready_kind,' || E'\n' ||
        '        database_now,' || E'\n' ||
        '        database_now,' || E'\n' ||
        '        2,';
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                previous_fragment,
                ''
            ))
        <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification ready attestation replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE function_definition;

    SELECT pg_catalog.jsonb_build_object(
        'oid', function_row.oid::TEXT,
        'owner', function_row.proowner::TEXT,
        'acl', pg_catalog.to_jsonb(function_row.proacl),
        'language', function_row.prolang::TEXT,
        'kind', function_row.prokind,
        'volatile', function_row.provolatile,
        'strict', function_row.proisstrict,
        'security_definer', function_row.prosecdef,
        'parallel', function_row.proparallel,
        'returns_set', function_row.proretset,
        'rows', function_row.prorows,
        'config', pg_catalog.to_jsonb(function_row.proconfig),
        'leakproof', function_row.proleakproof,
        'argument_defaults', function_row.pronargdefaults,
        'variadic', function_row.provariadic::TEXT,
        'return_type', function_row.prorettype::TEXT
    )
    INTO metadata_after
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    function_definition := pg_catalog.pg_get_functiondef(function_oid);
    IF metadata_after IS DISTINCT FROM metadata_before
        OR pg_catalog.strpos(
            function_definition,
            'route_record #>> ''{gateway,kind}'' IS DISTINCT FROM ''resumed'''
        ) <> 0
        OR pg_catalog.strpos(
            function_definition,
            'COALESCE(' || E'\n' ||
            '            route_record #>> ''{gateway,kind}'',' || E'\n' ||
            '            ''''' || E'\n' ||
            '        ) NOT IN (''ready'', ''resumed'')'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'deployment_row.snapshot -> ''gateway_ready''' || E'\n' ||
            '            IS DISTINCT FROM ''null''::JSONB'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'gateway_ready_kind := CASE route_record #>> ''{gateway,kind}'''
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            '''gateway_ready'', gateway_ready_value'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            '''{gateway_ready}'',' || E'\n' ||
            '        gateway_ready_value'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            '        gateway_ready_kind,' || E'\n' ||
            '        database_now,' || E'\n' ||
            '        database_now,' || E'\n' ||
            '        2,'
        ) = 0
    THEN
        RAISE EXCEPTION
            'runtime certification ready semantics replacement failed'
            USING ERRCODE = '55000';
    END IF;
END;
$runtime_certification_ready_semantics$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
