CREATE TYPE workspace_permission AS ENUM (
    'create_cards',
    'edit_cards',
    'delete_cards',
    'create_lists',
    'delete_lists',
    'create_labels',
    'delete_labels',
    'invite_members',
    'remove_members',
    'manage_permissions'
);

CREATE TABLE workspace_member_permissions (
    workspace_id UUID NOT NULL,
    user_id UUID NOT NULL,
    permission workspace_permission NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id, permission),
    FOREIGN KEY (workspace_id, user_id)
        REFERENCES workspace_members (workspace_id, user_id)
        ON DELETE CASCADE
);
