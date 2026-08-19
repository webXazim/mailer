CREATE TABLE webhook_endpoints (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    url text NOT NULL,
    signing_secret_hash bytea NOT NULL,
    subscriptions jsonb NOT NULL DEFAULT '[]',
    enabled boolean NOT NULL DEFAULT true,
    failure_count integer NOT NULL DEFAULT 0,
    last_success_at timestamptz,
    last_failure_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX webhook_endpoints_workspace_idx ON webhook_endpoints(workspace_id, enabled);

CREATE TABLE webhook_attempts (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    endpoint_id uuid NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event_id uuid NOT NULL REFERENCES delivery_events(id) ON DELETE CASCADE,
    attempt_number integer NOT NULL,
    status_code integer,
    response_body text,
    error text,
    next_retry_at timestamptz,
    delivered_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(endpoint_id, event_id, attempt_number)
);

CREATE TABLE templates (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    slug text NOT NULL,
    published_version_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(workspace_id, slug)
);

CREATE TABLE template_versions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    template_id uuid NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    version integer NOT NULL,
    subject text NOT NULL,
    html_body text,
    text_body text,
    variables jsonb NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(template_id, version)
);
ALTER TABLE templates ADD CONSTRAINT templates_published_version_fk FOREIGN KEY (published_version_id) REFERENCES template_versions(id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE suppressions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    address text NOT NULL,
    reason text NOT NULL CHECK (reason IN ('bounced', 'complained', 'unsubscribed', 'manual')),
    source_email_id uuid REFERENCES emails(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(workspace_id, address)
);
CREATE INDEX suppressions_workspace_reason_idx ON suppressions(workspace_id, reason, created_at DESC);

CREATE TABLE usage_counters (
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    period_start date NOT NULL,
    emails_accepted bigint NOT NULL DEFAULT 0,
    emails_delivered bigint NOT NULL DEFAULT 0,
    storage_bytes bigint NOT NULL DEFAULT 0,
    PRIMARY KEY(workspace_id, period_start)
);

CREATE TABLE audit_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    actor_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    action text NOT NULL,
    resource_type text,
    resource_id uuid,
    metadata jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_workspace_idx ON audit_events(workspace_id, created_at DESC);

CREATE TABLE billing_customers (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    provider_customer_id text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE billing_subscriptions (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    provider_subscription_id text NOT NULL UNIQUE,
    plan text NOT NULL,
    status text NOT NULL,
    current_period_start timestamptz,
    current_period_end timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now()
);
