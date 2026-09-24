-- Keep historical provider IDs on disabled Mailer domains for audit, while
-- allowing a newly verified claim to reuse that same Stalwart Domain object.
-- At most one non-disabled Mailer domain may bind to each provider object.
DROP INDEX domains_provider_identity_idx;
CREATE UNIQUE INDEX domains_provider_identity_idx
    ON domains (management_provider, provider_domain_id)
    WHERE provider_domain_id IS NOT NULL AND status <> 'disabled';
