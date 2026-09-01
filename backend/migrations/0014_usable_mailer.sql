-- Preserve old replay records, deriving their environment from the accepted mail.
ALTER TABLE idempotency_keys ADD COLUMN environment text;
UPDATE idempotency_keys k SET environment = e.environment
FROM emails e WHERE e.workspace_id = k.workspace_id AND e.id::text = k.response->'data'->>'id';
UPDATE idempotency_keys SET environment = 'production' WHERE environment IS NULL;
ALTER TABLE idempotency_keys ALTER COLUMN environment SET NOT NULL;
ALTER TABLE idempotency_keys ADD CONSTRAINT idempotency_environment CHECK (environment IN ('test','production'));
ALTER TABLE idempotency_keys DROP CONSTRAINT idempotency_keys_workspace_id_key_key;
ALTER TABLE idempotency_keys ADD UNIQUE (workspace_id, environment, key);

ALTER TABLE webhook_endpoints ADD COLUMN environment text NOT NULL DEFAULT 'production'
CHECK (environment IN ('test','production'));

CREATE TABLE account_emails (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    recipient text NOT NULL,
    subject text NOT NULL,
    body text NOT NULL,
    status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','processing','sent','failed')),
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    last_error text
);
ALTER TABLE account_emails ADD COLUMN provider_message_id text;
CREATE INDEX account_emails_due_idx ON account_emails(available_at) WHERE status = 'queued';
CREATE INDEX emails_workspace_environment_idx ON emails(workspace_id, environment, accepted_at DESC, id DESC);
