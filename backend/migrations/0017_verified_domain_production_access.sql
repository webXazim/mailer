-- A verified sending domain proves control of a sender identity and unlocks
-- self-service production access. This also upgrades workspaces whose domain
-- was verified before this behavior was introduced.
UPDATE workspaces AS workspace
SET production_enabled = true,
    updated_at = now()
WHERE NOT workspace.production_enabled
  AND EXISTS (
      SELECT 1
      FROM domains AS domain
      WHERE domain.workspace_id = workspace.id
        AND domain.status = 'verified'
  );
