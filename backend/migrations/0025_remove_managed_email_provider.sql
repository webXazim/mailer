-- Convert every legacy provider value to the only supported self-hosted path.
UPDATE emails
SET delivery_provider = 'smtp'
WHERE delivery_provider <> 'smtp';

UPDATE delivery_provider_attempts
SET provider = 'smtp'
WHERE provider <> 'smtp';

INSERT INTO delivery_provider_daily_usage (usage_date, provider, emails_admitted)
SELECT usage_date, 'smtp', SUM(emails_admitted)
FROM delivery_provider_daily_usage
WHERE provider <> 'smtp'
GROUP BY usage_date
ON CONFLICT (usage_date, provider) DO UPDATE
SET emails_admitted = delivery_provider_daily_usage.emails_admitted + EXCLUDED.emails_admitted;

DELETE FROM delivery_provider_daily_usage
WHERE provider <> 'smtp';

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
WHERE management_provider <> 'stalwart';

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

ALTER TABLE domains DROP COLUMN ses_identity_arn;

ALTER TABLE emails
    DROP CONSTRAINT emails_delivery_provider_check,
    ADD CONSTRAINT emails_delivery_provider_check CHECK (delivery_provider = 'smtp'),
    ALTER COLUMN delivery_provider SET DEFAULT 'smtp';

ALTER TABLE delivery_provider_attempts
    DROP CONSTRAINT delivery_provider_attempts_provider_check,
    ADD CONSTRAINT delivery_provider_attempts_provider_check CHECK (provider = 'smtp');

ALTER TABLE delivery_provider_daily_usage
    DROP CONSTRAINT delivery_provider_daily_usage_provider_check,
    ADD CONSTRAINT delivery_provider_daily_usage_provider_check CHECK (provider = 'smtp');

ALTER TABLE domains
    DROP CONSTRAINT domains_management_provider_check,
    ADD CONSTRAINT domains_management_provider_check CHECK (management_provider = 'stalwart'),
    ALTER COLUMN management_provider SET DEFAULT 'stalwart';
