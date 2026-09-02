-- Public accounts must verify their email and receive explicit operator approval
-- before they can consume the shared production SES account.
UPDATE users SET email_verified_at = created_at WHERE email_verified_at IS NULL;

ALTER TABLE workspaces ADD COLUMN production_enabled boolean NOT NULL DEFAULT false;
UPDATE workspaces SET production_enabled = true;

CREATE TABLE email_verification_tokens (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX email_verification_tokens_user_idx
    ON email_verification_tokens(user_id, expires_at) WHERE used_at IS NULL;
