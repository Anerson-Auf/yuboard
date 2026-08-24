-- Identity is deliberately independent from workspace membership.  The legacy
-- columns remain only to make an in-place upgrade non-destructive; application
-- code must never expose or accept them.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS is_system_owner BOOLEAN NOT NULL DEFAULT FALSE;

UPDATE users
SET username = lower(regexp_replace(display_name, '[^a-zA-Z0-9_.-]+', '_', 'g'))
WHERE username IS NULL
  AND display_name ~ '^[A-Za-z0-9_.-]{3,32}$';

UPDATE users
SET username = 'legacy_' || substr(replace(id::text, '-', ''), 1, 24)
WHERE username IS NULL;

ALTER TABLE users ALTER COLUMN username SET NOT NULL;
ALTER TABLE users ALTER COLUMN email DROP NOT NULL;
UPDATE users SET is_system_owner = TRUE WHERE is_system_admin = TRUE;
UPDATE users SET is_system_owner = TRUE
WHERE id = (SELECT id FROM users WHERE password_hash IS NOT NULL ORDER BY created_at, id LIMIT 1)
  AND NOT EXISTS (SELECT 1 FROM users WHERE is_system_owner);

ALTER TYPE workspace_role ADD VALUE IF NOT EXISTS 'contributor';
ALTER TYPE workspace_role ADD VALUE IF NOT EXISTS 'editor';
ALTER TYPE workspace_role ADD VALUE IF NOT EXISTS 'full_access';

ALTER TABLE workspace_invitations RENAME TO legacy_workspace_invitations;

ALTER TABLE account_invitations
    ADD COLUMN IF NOT EXISTS revoked_at TIMESTAMPTZ;

ALTER TABLE workspace_member_permissions
    ADD COLUMN IF NOT EXISTS granted_by UUID REFERENCES users(id),
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE TABLE IF NOT EXISTS audit_log (
    id UUID PRIMARY KEY,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    target_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (char_length(action) BETWEEN 1 AND 100),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS audit_log_workspace_created_idx ON audit_log (workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS audit_log_target_created_idx ON audit_log (target_user_id, created_at DESC);

-- The enum values above become usable in the following migration. PostgreSQL
-- deliberately rejects using a newly added enum value in the same migration.
