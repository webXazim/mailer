-- Convert the retired managed-provider configuration to the self-hosted SMTP path.
-- Historical submitted attempts retain their original provider for audit accuracy.
UPDATE emails
SET delivery_provider = 'smtp'
WHERE delivery_provider = 'ses'
  AND status IN ('queued', 'processing');

UPDATE domains
SET management_provider = 'stalwart',
    provider_status = 'pending',
    provider_domain_id = NULL,
    active_dkim_signature_id = NULL,
    active_dkim_selector = NULL,
    previous_dkim_signature_id = NULL,
    previous_dkim_record_name = NULL,
    pending_dkim_selector = NULL,
    status = 'pending',
    verified_at = NULL,
    updated_at = now()
WHERE management_provider = 'ses';

DELETE FROM domain_dns_records
WHERE value ILIKE '%amazonses.com%';

UPDATE delivery_operator_controls
SET smtp_paused = false,
    ses_rollback_enabled = false,
    default_provider = 'smtp',
    updated_at = now();

DELETE FROM workspace_delivery_routes;

DROP TABLE workspace_delivery_routes;
ALTER TABLE delivery_operator_controls
    DROP COLUMN ses_rollback_enabled,
    DROP COLUMN default_provider;

ALTER TABLE emails ALTER COLUMN delivery_provider SET DEFAULT 'smtp';
ALTER TABLE domains ALTER COLUMN management_provider SET DEFAULT 'stalwart';
