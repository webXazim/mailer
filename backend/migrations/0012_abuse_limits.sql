CREATE TABLE api_key_rate_limits (
    api_key_id uuid NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    bucket_start timestamptz NOT NULL,
    request_count integer NOT NULL DEFAULT 0,
    PRIMARY KEY (api_key_id, bucket_start)
);
CREATE INDEX api_key_rate_limits_cleanup_idx ON api_key_rate_limits(bucket_start);

CREATE TABLE client_ip_rate_limits (
    client_ip text NOT NULL,
    bucket_start timestamptz NOT NULL,
    request_count integer NOT NULL DEFAULT 0,
    PRIMARY KEY (client_ip, bucket_start)
);
CREATE INDEX client_ip_rate_limits_cleanup_idx ON client_ip_rate_limits(bucket_start);

CREATE INDEX emails_workspace_active_idx
ON emails(workspace_id)
WHERE status IN ('queued', 'processing');

CREATE TABLE workspace_limits (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    monthly_email_limit bigint CHECK (monthly_email_limit > 0),
    concurrent_email_limit integer CHECK (concurrent_email_limit > 0),
    api_key_rate_limit_per_minute integer CHECK (api_key_rate_limit_per_minute > 0),
    updated_at timestamptz NOT NULL DEFAULT now()
);
