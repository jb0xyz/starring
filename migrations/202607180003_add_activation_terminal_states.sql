ALTER TABLE activation_requests
ADD COLUMN termination JSONB;

ALTER TABLE activation_requests
DROP CONSTRAINT activation_requests_state_valid,
ADD CONSTRAINT activation_requests_state_valid
CHECK (
    state IN (
        'pending',
        'approved',
        'applying',
        'applied',
        'rejected',
        'expired',
        'superseded',
        'withdrawn'
    )
),
ADD CONSTRAINT activation_requests_termination_object
CHECK (termination IS NULL OR jsonb_typeof(termination) = 'object'),
ADD CONSTRAINT activation_requests_termination_state_valid
CHECK (
    (state = 'superseded' AND (termination ->> 'kind') = 'superseded')
    OR (state = 'withdrawn' AND (termination ->> 'kind') = 'withdrawn')
    OR (state NOT IN ('superseded','withdrawn') AND termination IS NULL)
);
