ALTER TABLE emails
    ADD COLUMN delivery_provider text NOT NULL DEFAULT 'ses'
    CHECK (delivery_provider IN ('ses', 'smtp'));

CREATE INDEX emails_delivery_provider_status_idx
    ON emails(delivery_provider, status, accepted_at)
    WHERE environment = 'production' AND status IN ('queued', 'processing');

CREATE TABLE delivery_provider_attempts (
    id uuid PRIMARY KEY,
    email_id uuid NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (provider IN ('ses', 'smtp')),
    attempt_number integer NOT NULL CHECK (attempt_number > 0),
    status text NOT NULL DEFAULT 'processing'
        CHECK (status IN ('processing', 'submitted', 'retryable', 'ambiguous', 'failed')),
    provider_message_id text,
    error text,
    started_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE(email_id, attempt_number)
);

CREATE INDEX delivery_provider_attempts_email_idx
    ON delivery_provider_attempts(email_id, started_at DESC);

CREATE INDEX delivery_provider_attempts_provider_message_idx
    ON delivery_provider_attempts(provider, provider_message_id)
    WHERE provider_message_id IS NOT NULL;
