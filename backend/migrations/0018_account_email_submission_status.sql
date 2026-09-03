ALTER TABLE account_emails
    DROP CONSTRAINT account_emails_status_check,
    ADD CONSTRAINT account_emails_status_check
        CHECK (status IN ('queued','processing','submitted','sent','failed')),
    ADD COLUMN mailer_email_id uuid REFERENCES emails(id) ON DELETE SET NULL;
