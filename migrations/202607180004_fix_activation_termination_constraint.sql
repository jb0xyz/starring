ALTER TABLE activation_requests
DROP CONSTRAINT activation_requests_termination_state_valid,
ADD CONSTRAINT activation_requests_termination_state_valid
CHECK (
    (
        (state = 'superseded' AND (termination ->> 'kind') = 'superseded')
        OR (state = 'withdrawn' AND (termination ->> 'kind') = 'withdrawn')
        OR (state NOT IN ('superseded','withdrawn') AND termination IS NULL)
    ) IS TRUE
);
