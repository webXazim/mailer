-- Existing Mailer domains created before ownership markers need a guarded
-- Stalwart reconciliation. New domains are marked at creation time.
ALTER TABLE domains
    ADD COLUMN provider_marker_checked_at timestamptz,
    ADD COLUMN provider_marker_next_check_at timestamptz NOT NULL DEFAULT now();

CREATE INDEX domains_legacy_marker_due_idx ON domains(provider_marker_next_check_at)
    WHERE status = 'verified' AND management_provider = 'stalwart'
      AND shared_stalwart_domain = false AND provider_domain_id IS NOT NULL
      AND provider_marker_checked_at IS NULL;
