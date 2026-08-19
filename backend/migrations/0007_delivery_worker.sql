ALTER TABLE emails ADD COLUMN processing_started_at timestamptz;
ALTER TABLE emails ADD COLUMN processing_attempts integer NOT NULL DEFAULT 0;
ALTER TABLE emails ADD COLUMN last_error text;

ALTER TABLE outbox_events ADD COLUMN last_error text;

CREATE INDEX emails_stale_processing_idx ON emails(processing_started_at)
WHERE status = 'processing';
