CREATE TABLE authoring_promotions (
    id TEXT PRIMARY KEY,
    record_format_version SMALLINT NOT NULL,
    revision BIGINT NOT NULL,
    stage TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    record JSONB NOT NULL,
    CONSTRAINT authoring_promotions_id_format CHECK (id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT authoring_promotions_record_format_valid CHECK (record_format_version = 1),
    CONSTRAINT authoring_promotions_revision_valid CHECK (revision BETWEEN 1 AND 9223372036854775807),
    CONSTRAINT authoring_promotions_stage_valid CHECK (stage IN ('prepared','published','activation_pending','expired')),
    CONSTRAINT authoring_promotions_stage_revision_valid CHECK (
        (stage = 'prepared' AND revision = 1)
        OR (stage = 'published' AND revision = 2)
        OR (stage IN ('activation_pending','expired') AND revision = 3)
    ),
    CONSTRAINT authoring_promotions_request_digest_format CHECK (request_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT authoring_promotions_tenant_id_format CHECK (tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    CONSTRAINT authoring_promotions_principal_id_format CHECK (principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    CONSTRAINT authoring_promotions_record_valid CHECK (jsonb_typeof(record) = 'object'),
    CONSTRAINT authoring_promotions_record_identity_valid CHECK (
        ((record ->> 'id') = id) IS TRUE
        AND ((record ->> 'request_digest') = request_digest) IS TRUE
        AND ((record ->> 'revision') = revision::TEXT) IS TRUE
        AND ((record -> 'stage' ->> 'state') = stage) IS TRUE
        AND ((record -> 'intent' -> 'authority' ->> 'tenant_id') = tenant_id) IS TRUE
        AND ((record -> 'intent' -> 'authority' ->> 'principal_id') = principal_id) IS TRUE
    )
);

CREATE INDEX authoring_promotions_authority_index
ON authoring_promotions (tenant_id, principal_id);
