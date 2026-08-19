CREATE UNIQUE INDEX emails_provider_message_idx ON emails(provider_message_id)
WHERE provider_message_id IS NOT NULL;

ALTER TABLE suppressions DROP CONSTRAINT suppressions_workspace_id_address_key;
CREATE UNIQUE INDEX suppressions_workspace_address_lower_idx
ON suppressions(workspace_id, lower(address));
