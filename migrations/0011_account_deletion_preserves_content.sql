-- Preserve work history while allowing an account to be physically removed.
ALTER TABLE workspaces ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE workspaces DROP CONSTRAINT IF EXISTS workspaces_created_by_fkey;
ALTER TABLE workspaces ADD CONSTRAINT workspaces_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE boards ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE boards DROP CONSTRAINT IF EXISTS boards_created_by_fkey;
ALTER TABLE boards ADD CONSTRAINT boards_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE cards ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE cards DROP CONSTRAINT IF EXISTS cards_created_by_fkey;
ALTER TABLE cards ADD CONSTRAINT cards_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE comments ALTER COLUMN author_id DROP NOT NULL;
ALTER TABLE comments DROP CONSTRAINT IF EXISTS comments_author_id_fkey;
ALTER TABLE comments ADD CONSTRAINT comments_author_id_fkey FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE attachments ALTER COLUMN uploaded_by DROP NOT NULL;
ALTER TABLE attachments DROP CONSTRAINT IF EXISTS attachments_uploaded_by_fkey;
ALTER TABLE attachments ADD CONSTRAINT attachments_uploaded_by_fkey FOREIGN KEY (uploaded_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE account_invitations ALTER COLUMN invited_by DROP NOT NULL;
ALTER TABLE account_invitations DROP CONSTRAINT IF EXISTS account_invitations_invited_by_fkey;
ALTER TABLE account_invitations ADD CONSTRAINT account_invitations_invited_by_fkey FOREIGN KEY (invited_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE card_diagrams ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE card_diagrams DROP CONSTRAINT IF EXISTS card_diagrams_created_by_fkey;
ALTER TABLE card_diagrams ADD CONSTRAINT card_diagrams_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE workspace_member_permissions DROP CONSTRAINT IF EXISTS workspace_member_permissions_granted_by_fkey;
ALTER TABLE workspace_member_permissions ADD CONSTRAINT workspace_member_permissions_granted_by_fkey FOREIGN KEY (granted_by) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE legacy_workspace_invitations ALTER COLUMN invited_by DROP NOT NULL;
ALTER TABLE legacy_workspace_invitations DROP CONSTRAINT IF EXISTS workspace_invitations_invited_by_fkey;
ALTER TABLE legacy_workspace_invitations DROP CONSTRAINT IF EXISTS legacy_workspace_invitations_invited_by_fkey;
ALTER TABLE legacy_workspace_invitations ADD CONSTRAINT legacy_workspace_invitations_invited_by_fkey FOREIGN KEY (invited_by) REFERENCES users(id) ON DELETE SET NULL;
