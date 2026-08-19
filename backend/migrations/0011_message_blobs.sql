ALTER TABLE emails ADD COLUMN content_checksum bytea;
ALTER TABLE emails ADD COLUMN content_deleted_at timestamptz;

ALTER TABLE emails ADD CONSTRAINT emails_content_storage_check CHECK (
    (raw_object_key IS NULL AND content_checksum IS NULL)
    OR (raw_object_key IS NOT NULL AND content_checksum IS NOT NULL)
);
