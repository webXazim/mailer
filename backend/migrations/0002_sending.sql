CREATE TABLE domains (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'verified', 'failed', 'disabled')),
    ses_identity_arn text,
    verified_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(workspace_id, name)
);

CREATE TABLE domain_dns_records (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    domain_id uuid NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    record_type text NOT NULL CHECK (record_type IN ('SPF', 'DKIM', 'DMARC', 'MX', 'CNAME', 'TXT')),
    name text NOT NULL,
    value text NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'verified', 'failed')),
    last_checked_at timestamptz,
    UNIQUE(domain_id, record_type, name)
);

CREATE TABLE api_keys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    key_prefix text NOT NULL,
    secret_hash bytea NOT NULL UNIQUE,
    environment text NOT NULL CHECK (environment IN ('test', 'production')),
    scopes jsonb NOT NULL DEFAULT '[]',
    expires_at timestamptz,
    revoked_at timestamptz,
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX api_keys_workspace_active_idx ON api_keys(workspace_id, environment) WHERE revoked_at IS NULL;

CREATE TABLE emails (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
    domain_id uuid REFERENCES domains(id) ON DELETE RESTRICT,
    idempotency_key text,
    environment text NOT NULL CHECK (environment IN ('test', 'production')),
    sender text NOT NULL,
    subject text NOT NULL,
    text_body text,
    html_body text,
    raw_object_key text,
    status text NOT NULL DEFAULT 'accepted' CHECK (status IN ('accepted', 'queued', 'processing', 'sent', 'delivered', 'bounced', 'complained', 'failed', 'cancelled')),
    provider_message_id text,
    metadata jsonb NOT NULL DEFAULT '{}',
    accepted_at timestamptz NOT NULL DEFAULT now(),
    sent_at timestamptz,
    completed_at timestamptz
);
CREATE UNIQUE INDEX emails_idempotency_idx ON emails(workspace_id, environment, idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE INDEX emails_workspace_status_idx ON emails(workspace_id, status, accepted_at DESC);

CREATE TABLE email_recipients (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email_id uuid NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    address text NOT NULL,
    recipient_type text NOT NULL CHECK (recipient_type IN ('to', 'cc', 'bcc')),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'delivered', 'bounced', 'complained', 'failed')),
    UNIQUE(email_id, address, recipient_type)
);

CREATE TABLE delivery_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email_id uuid REFERENCES emails(id) ON DELETE SET NULL,
    provider_event_id text NOT NULL UNIQUE,
    event_type text NOT NULL,
    recipient text,
    payload jsonb NOT NULL,
    occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX delivery_events_email_idx ON delivery_events(email_id, occurred_at DESC);

CREATE TABLE idempotency_keys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    key text NOT NULL,
    request_hash bytea NOT NULL,
    response jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(workspace_id, key)
);

CREATE TABLE outbox_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    aggregate_type text NOT NULL,
    aggregate_id uuid NOT NULL,
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    available_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz,
    attempts integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX outbox_unpublished_idx ON outbox_events(available_at, created_at) WHERE published_at IS NULL;
