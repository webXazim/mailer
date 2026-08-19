ALTER TABLE webhook_endpoints
ADD COLUMN signing_secret_version integer NOT NULL DEFAULT 1,
ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();

CREATE TABLE webhook_deliveries (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    endpoint_id uuid NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event_id uuid NOT NULL REFERENCES delivery_events(id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'succeeded', 'failed')),
    attempts integer NOT NULL DEFAULT 0,
    total_attempts integer NOT NULL DEFAULT 0,
    retry_generation integer NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    last_error text,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(endpoint_id, event_id)
);
CREATE INDEX webhook_deliveries_pending_idx
ON webhook_deliveries(next_attempt_at, created_at)
WHERE status = 'pending';

CREATE TABLE webhook_dead_letters (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    delivery_id uuid NOT NULL REFERENCES webhook_deliveries(id) ON DELETE CASCADE,
    reason text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX webhook_dead_letters_delivery_idx
ON webhook_dead_letters(delivery_id, created_at DESC);
