CREATE TABLE delivery_operator_controls (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    smtp_paused boolean NOT NULL DEFAULT true,
    smtp_daily_email_limit bigint NOT NULL DEFAULT 100 CHECK (smtp_daily_email_limit > 0),
    ses_rollback_enabled boolean NOT NULL DEFAULT true,
    updated_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO delivery_operator_controls (singleton) VALUES (true);

CREATE TABLE workspace_delivery_routes (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (provider IN ('ses', 'smtp')),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE delivery_provider_daily_usage (
    usage_date date NOT NULL,
    provider text NOT NULL CHECK (provider IN ('ses', 'smtp')),
    emails_admitted bigint NOT NULL DEFAULT 0 CHECK (emails_admitted >= 0),
    PRIMARY KEY (usage_date, provider)
);

CREATE TABLE delivery_control_audit (
    id bigserial PRIMARY KEY,
    action text NOT NULL,
    details jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX delivery_control_audit_created_idx
    ON delivery_control_audit(created_at DESC);

CREATE INDEX workspace_delivery_routes_provider_idx
    ON workspace_delivery_routes(provider, workspace_id);
