-- A Discord integration is deliberately scoped to one board and one target list.
-- The raw token is returned once and only its SHA-256 digest is stored.
CREATE TABLE discord_integrations (
    id UUID PRIMARY KEY,
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    target_list_id UUID NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    token_hash BYTEA NOT NULL UNIQUE,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);

CREATE INDEX discord_integrations_board_active_idx ON discord_integrations (board_id, created_at DESC) WHERE revoked_at IS NULL;

-- A Discord identity is external content, not a Flowboard account.  We keep it
-- on the historical comment so deleting a Flowboard user still means "Deleted user".
ALTER TABLE comments
    ADD COLUMN external_author_name TEXT,
    ADD COLUMN external_author_avatar_url TEXT,
    ADD COLUMN discord_integration_id UUID REFERENCES discord_integrations(id) ON DELETE SET NULL,
    ADD COLUMN discord_message_id TEXT;

ALTER TABLE cards
    ADD COLUMN discord_integration_id UUID REFERENCES discord_integrations(id) ON DELETE SET NULL,
    ADD COLUMN discord_source_id TEXT;

CREATE UNIQUE INDEX cards_discord_source_idx ON cards (discord_integration_id, discord_source_id) WHERE discord_integration_id IS NOT NULL AND discord_source_id IS NOT NULL;
CREATE UNIQUE INDEX comments_discord_message_idx ON comments (discord_integration_id, discord_message_id) WHERE discord_integration_id IS NOT NULL AND discord_message_id IS NOT NULL;
