ALTER TABLE emails ADD COLUMN api_key_id uuid REFERENCES api_keys(id) ON DELETE SET NULL;
ALTER TABLE emails ADD COLUMN reply_to text;
ALTER TABLE emails ADD COLUMN headers jsonb NOT NULL DEFAULT '{}';
ALTER TABLE emails ADD COLUMN tags jsonb NOT NULL DEFAULT '[]';

CREATE INDEX emails_workspace_accepted_idx ON emails(workspace_id, accepted_at DESC);
CREATE INDEX email_recipients_address_idx ON email_recipients(lower(address));
