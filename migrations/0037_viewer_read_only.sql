-- A viewer is strictly read-only.  Earlier versions allowed a role downgrade
-- to leave custom workspace permissions behind, effectively making a viewer
-- an editor.  Drop those stale grants and make the policy deny writes before
-- evaluating any remaining manual permissions.
DELETE FROM workspace_member_permissions permission
USING workspace_members member
WHERE member.workspace_id = permission.workspace_id
  AND member.user_id = permission.user_id
  AND member.role = 'viewer';

CREATE OR REPLACE FUNCTION flowboard_has_permission(
    requested_workspace UUID,
    requested_user UUID,
    requested_permission workspace_permission
) RETURNS BOOLEAN
LANGUAGE SQL STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM users user_account
        WHERE user_account.id = requested_user
          AND user_account.disabled_at IS NULL
          AND user_account.is_system_owner
    ) OR EXISTS (
        SELECT 1 FROM workspace_members member
        WHERE member.workspace_id = requested_workspace
          AND member.user_id = requested_user
          AND member.role <> 'viewer'
          AND (
              member.role IN ('owner', 'full_access')
              OR (member.role = 'editor' AND requested_permission IN (
                  'create_cards', 'edit_cards', 'delete_cards', 'create_lists',
                  'delete_lists', 'create_labels', 'delete_labels'
              ))
              OR (member.role = 'contributor' AND requested_permission IN ('create_cards', 'edit_cards'))
              OR EXISTS (
                  SELECT 1 FROM workspace_member_permissions permission
                  WHERE permission.workspace_id = member.workspace_id
                    AND permission.user_id = member.user_id
                    AND permission.permission = requested_permission
              )
          )
    );
$$;
