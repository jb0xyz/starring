SET LOCAL search_path = pg_catalog, public;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE public.ruleset_panel_installations
ADD CONSTRAINT ruleset_panel_installations_guild_id_bounded
CHECK (
    CASE
        WHEN guild_id ~ '^[1-9][0-9]{0,19}$'
        THEN guild_id::NUMERIC <= 18446744073709551615
        ELSE FALSE
    END
) NOT VALID,
ADD CONSTRAINT ruleset_panel_installations_ruleset_key_bounded
CHECK (
    octet_length(ruleset_key) BETWEEN 1 AND 64
    AND (ruleset_key COLLATE "C") ~ '^[A-Za-z0-9_-]+$'
) NOT VALID,
ADD CONSTRAINT ruleset_panel_installations_panel_key_bounded
CHECK (octet_length(panel_key) BETWEEN 1 AND 128) NOT VALID,
ADD CONSTRAINT ruleset_panel_installations_channel_id_bounded
CHECK (
    CASE
        WHEN channel_id ~ '^[1-9][0-9]{0,19}$'
        THEN channel_id::NUMERIC <= 18446744073709551615
        ELSE FALSE
    END
) NOT VALID,
ADD CONSTRAINT ruleset_panel_installations_message_id_bounded
CHECK (
    CASE
        WHEN message_id ~ '^[1-9][0-9]{0,19}$'
        THEN message_id::NUMERIC <= 18446744073709551615
        ELSE FALSE
    END
) NOT VALID;

CREATE TABLE public.strict_panel_operation_journal (
    record_format_version SMALLINT NOT NULL,
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    panel_key TEXT NOT NULL,
    state_tag TEXT NOT NULL,
    operation_payload JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (guild_id, ruleset_key, panel_key),
    CONSTRAINT strict_panel_operation_journal_format_valid CHECK (
        record_format_version = 1
    ),
    CONSTRAINT strict_panel_operation_journal_guild_id_bounded CHECK (
        CASE
            WHEN guild_id ~ '^[1-9][0-9]{0,19}$'
            THEN guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    ),
    CONSTRAINT strict_panel_operation_journal_ruleset_key_bounded CHECK (
        octet_length(ruleset_key) BETWEEN 1 AND 64
        AND (ruleset_key COLLATE "C") ~ '^[A-Za-z0-9_-]+$'
    ),
    CONSTRAINT strict_panel_operation_journal_panel_key_bounded CHECK (
        octet_length(panel_key) BETWEEN 1 AND 128
    ),
    CONSTRAINT strict_panel_operation_journal_state_valid CHECK (
        state_tag IN (
            'post_dispatching',
            'post_applied',
            'ambiguous_post',
            'cleanup_pending'
        )
    ),
    CONSTRAINT strict_panel_operation_journal_payload_bounded CHECK (
        (
            jsonb_typeof(operation_payload) = 'object'
            AND octet_length(operation_payload::TEXT) BETWEEN 32 AND 262144
        ) IS TRUE
    ),
    CONSTRAINT strict_panel_operation_journal_payload_key_matches CHECK (
        (
            operation_payload #>> '{key,guild_id}' = guild_id
            AND operation_payload #>> '{key,ruleset_key}' = ruleset_key
            AND operation_payload #>> '{key,panel_key}' = panel_key
        ) IS TRUE
    ),
    CONSTRAINT strict_panel_operation_journal_payload_state_matches CHECK (
        (operation_payload #>> '{state,state}' = state_tag) IS TRUE
    )
);
