ALTER TABLE automation_instances ADD COLUMN ruleset_version BIGINT;

UPDATE automation_instances SET ruleset_version = 1 WHERE status = 'deleted';

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM automation_instances WHERE ruleset_version IS NULL) THEN
        RAISE EXCEPTION
            'non-deleted legacy automation instances require an explicit ruleset version';
    END IF;
END
$$;

ALTER TABLE automation_instances ALTER COLUMN ruleset_version SET NOT NULL;

ALTER TABLE automation_instances
    ADD CONSTRAINT automation_instances_ruleset_version_valid
    CHECK (ruleset_version BETWEEN 1 AND 4294967295);
