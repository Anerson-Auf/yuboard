-- A workspace is a container; access to its projects is granted explicitly.
-- Preserve broad access only for workspace-wide owners and full-access users.
-- Other legacy memberships are intentionally denied until they are granted to
-- a concrete project: the old data contains no information about which one.
INSERT INTO board_members (board_id, user_id, role)
SELECT
    b.id,
    wm.user_id,
    CASE WHEN wm.role = 'viewer' THEN 'viewer'::board_role ELSE 'editor'::board_role END
FROM boards b
JOIN workspace_members wm ON wm.workspace_id = b.workspace_id
WHERE b.archived_at IS NULL AND wm.role IN ('owner', 'full_access')
ON CONFLICT (board_id, user_id) DO NOTHING;

CREATE INDEX IF NOT EXISTS board_members_user_idx ON board_members (user_id, board_id);
