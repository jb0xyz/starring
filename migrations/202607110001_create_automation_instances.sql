CREATE TABLE automation_instances (
    guild_id    TEXT NOT NULL,
    instance_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    kind        TEXT NOT NULL,
    created_by  TEXT NOT NULL,
    status      TEXT NOT NULL,
    resources   JSONB NOT NULL,
    PRIMARY KEY (guild_id, instance_id),
    CONSTRAINT automation_instances_instance_id_format CHECK (instance_id ~ '^[A-Za-z0-9_-]{1,32}$'),
    CONSTRAINT automation_instances_status_valid CHECK (status IN ('active','disabled','deleted')),
    CONSTRAINT automation_instances_resources_object CHECK (jsonb_typeof(resources) = 'object')
);
