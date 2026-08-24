UPDATE workspace_members SET role = 'contributor' WHERE role = 'member';
UPDATE workspace_members SET role = 'full_access' WHERE role = 'admin';

CREATE OR REPLACE FUNCTION flowboard_has_permission(
    requested_workspace UUID,
    requested_user UUID,
    requested_permission workspace_permission
) RETURNS BOOLEAN
LANGUAGE SQL STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM users u
        WHERE u.id = requested_user AND u.disabled_at IS NULL AND u.is_system_owner
    ) OR EXISTS (
        SELECT 1 FROM workspace_members wm
        WHERE wm.workspace_id = requested_workspace AND wm.user_id = requested_user
          AND (
              wm.role IN ('owner', 'full_access')
              OR (wm.role = 'editor' AND requested_permission IN (
                  'create_cards', 'edit_cards', 'delete_cards', 'create_lists',
                  'delete_lists', 'create_labels', 'delete_labels'
              ))
              OR (wm.role = 'contributor' AND requested_permission IN ('create_cards', 'edit_cards'))
              OR EXISTS (
                  SELECT 1 FROM workspace_member_permissions p
                  WHERE p.workspace_id = wm.workspace_id
                    AND p.user_id = wm.user_id
                    AND p.permission = requested_permission
              )
          )
    );
$$;
