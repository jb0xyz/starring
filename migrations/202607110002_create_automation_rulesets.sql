CREATE TABLE automation_ruleset_heads (
    guild_id     TEXT NOT NULL,
    ruleset_key  TEXT NOT NULL,
    next_version BIGINT NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key),
    CONSTRAINT arh_key_format CHECK (ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'),
    CONSTRAINT arh_next_range CHECK (next_version BETWEEN 1 AND 4294967296)
);

CREATE TABLE automation_ruleset_versions (
    guild_id       TEXT NOT NULL,
    ruleset_key    TEXT NOT NULL,
    version        BIGINT NOT NULL,
    schema_version BIGINT NOT NULL,
    definition     JSONB NOT NULL,
    content_hash   TEXT NOT NULL,
    created_by     TEXT NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key, version),
    CONSTRAINT arv_hash_unique UNIQUE (guild_id, ruleset_key, content_hash),
    CONSTRAINT arv_key_format CHECK (ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'),
    CONSTRAINT arv_hash_format CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT arv_version_range CHECK (version BETWEEN 1 AND 4294967295),
    CONSTRAINT arv_schema_range CHECK (schema_version BETWEEN 1 AND 4294967295),
    CONSTRAINT arv_definition_object CHECK (jsonb_typeof(definition) = 'object')
);

CREATE TABLE automation_ruleset_activations (
    guild_id       TEXT NOT NULL,
    ruleset_key    TEXT NOT NULL,
    active_version BIGINT NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key),
    CONSTRAINT ara_fk FOREIGN KEY (guild_id, ruleset_key, active_version)
        REFERENCES automation_ruleset_versions (guild_id, ruleset_key, version)
        ON DELETE RESTRICT
);
