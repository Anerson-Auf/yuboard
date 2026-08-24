ALTER TABLE users
    ADD COLUMN username TEXT,
    ADD COLUMN is_system_admin BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX users_username_unique_idx
    ON users (lower(username))
    WHERE username IS NOT NULL;

CREATE TABLE account_invitations (
    id UUID PRIMARY KEY,
    token_hash BYTEA NOT NULL UNIQUE,
    invited_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ
);

CREATE INDEX account_invitations_open_idx
    ON account_invitations (expires_at)
    WHERE accepted_at IS NULL;
