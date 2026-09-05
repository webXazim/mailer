ALTER TABLE workspaces
    ADD COLUMN sending_paused_at timestamptz,
    ADD COLUMN sending_pause_reason text,
    ADD COLUMN sending_paused_by text;

ALTER TABLE workspaces
    ADD CONSTRAINT workspaces_sending_pause_consistent CHECK (
        (sending_paused_at IS NULL AND sending_pause_reason IS NULL AND sending_paused_by IS NULL)
        OR
        (sending_paused_at IS NOT NULL AND sending_pause_reason IS NOT NULL AND sending_paused_by IS NOT NULL)
    );

CREATE INDEX audit_events_security_idx
ON audit_events(workspace_id, created_at DESC)
WHERE action LIKE 'security.%';
