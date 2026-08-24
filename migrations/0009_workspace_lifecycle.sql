ALTER TABLE workspaces ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS workspaces_active_idx ON workspaces (created_at DESC) WHERE archived_at IS NULL;

-- System owners are explicit members of every workspace so legacy read paths
-- and all future resource checks share one authorization boundary.
INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT w.id, u.id, 'owner'
FROM workspaces w
CROSS JOIN users u
WHERE u.is_system_owner
ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = 'owner';
