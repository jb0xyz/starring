ALTER TABLE authoring_promotions
    DROP CONSTRAINT authoring_promotions_stage_revision_valid;

ALTER TABLE authoring_promotions
    ADD CONSTRAINT authoring_promotions_stage_revision_valid CHECK (
        (stage = 'prepared' AND revision = 1)
        OR (stage = 'published' AND revision = 2)
        OR (stage = 'activation_pending' AND revision = 3)
        OR (stage = 'expired' AND revision IN (3, 4))
    );
