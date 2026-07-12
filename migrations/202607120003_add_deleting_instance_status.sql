ALTER TABLE automation_instances
    DROP CONSTRAINT automation_instances_status_valid;

ALTER TABLE automation_instances
    ADD CONSTRAINT automation_instances_status_valid
    CHECK (status IN ('active','deleting','disabled','deleted'));
