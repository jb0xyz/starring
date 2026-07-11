CREATE TABLE IF NOT EXISTS ruleset_panel_installations (
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    panel_key TEXT NOT NULL,
    installed_version BIGINT NOT NULL CHECK (installed_version BETWEEN 1 AND 4294967295),
    channel_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    spec_hash TEXT NOT NULL CHECK (spec_hash ~ '^[0-9a-f]{64}$'),
    PRIMARY KEY (guild_id, ruleset_key, panel_key)
);
