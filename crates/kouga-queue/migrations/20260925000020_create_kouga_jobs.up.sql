CREATE TABLE kouga_jobs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    queue text NOT NULL,
    payload jsonb NOT NULL,
    available_at timestamptz NOT NULL,
    attempt integer NOT NULL DEFAULT 0,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'succeeded', 'dead', 'cancelled', 'quarantined')),
    lease_token uuid,
    lease_until timestamptz,
    traceparent text,
    tracestate text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX kouga_jobs_ready ON kouga_jobs (queue, available_at) WHERE status = 'pending';
