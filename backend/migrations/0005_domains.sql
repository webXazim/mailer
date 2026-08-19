ALTER TABLE domain_dns_records ADD COLUMN required_for_sending boolean NOT NULL DEFAULT true;
ALTER TABLE domains ADD COLUMN provider_status text NOT NULL DEFAULT 'pending' CHECK (provider_status IN ('pending', 'verified', 'failed'));
ALTER TABLE domains DROP CONSTRAINT domains_workspace_id_name_key;
CREATE UNIQUE INDEX domains_name_global_idx ON domains (lower(name)) WHERE status <> 'disabled';
