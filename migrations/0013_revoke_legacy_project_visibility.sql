-- Migration 0012 previously backfilled every workspace member. A legacy
-- workspace membership cannot prove access to a particular project, so remove
-- those broad viewer/editor grants. Owners and full-access users retain their
-- workspace-wide visibility.
DELETE FROM board_members bm
USING boards b, workspace_members wm
WHERE bm.board_id = b.id
  AND wm.workspace_id = b.workspace_id
  AND wm.user_id = bm.user_id
  AND wm.role NOT IN ('owner', 'full_access');
