CREATE TABLE dns_oauth_states (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    domain_id uuid NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (provider IN ('cloudflare')),
    state_hash bytea NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX dns_oauth_states_active_idx
    ON dns_oauth_states(expires_at) WHERE used_at IS NULL;
