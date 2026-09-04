ALTER TABLE domains
    ADD COLUMN management_provider text NOT NULL DEFAULT 'ses'
        CHECK (management_provider IN ('ses', 'stalwart')),
    ADD COLUMN provider_domain_id text,
    ADD COLUMN active_dkim_signature_id text,
    ADD COLUMN active_dkim_selector text,
    ADD COLUMN previous_dkim_signature_id text,
    ADD COLUMN previous_dkim_record_name text,
    ADD COLUMN pending_dkim_selector text;

CREATE UNIQUE INDEX domains_provider_identity_idx
    ON domains (management_provider, provider_domain_id)
    WHERE provider_domain_id IS NOT NULL;
