-- Finish the provider cleanup without changing the already-applied 0025 migration.
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

ALTER TABLE domains DROP COLUMN ses_identity_arn;

ALTER TABLE emails
    DROP CONSTRAINT emails_delivery_provider_check,
    ADD CONSTRAINT emails_delivery_provider_check CHECK (delivery_provider = 'smtp');

ALTER TABLE delivery_provider_attempts
    DROP CONSTRAINT delivery_provider_attempts_provider_check,
    ADD CONSTRAINT delivery_provider_attempts_provider_check CHECK (provider = 'smtp');

ALTER TABLE delivery_provider_daily_usage
    DROP CONSTRAINT delivery_provider_daily_usage_provider_check,
    ADD CONSTRAINT delivery_provider_daily_usage_provider_check CHECK (provider = 'smtp');

ALTER TABLE domains
    DROP CONSTRAINT domains_management_provider_check,
    ADD CONSTRAINT domains_management_provider_check CHECK (management_provider = 'stalwart');
