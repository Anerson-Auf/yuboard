-- A Discord identity is optional profile metadata. It is global to a Flowboard
-- account, while bridge lookups remain scoped to members of one board.
CREATE TABLE IF NOT EXISTS user_discord_accounts (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    discord_user_id TEXT NOT NULL UNIQUE,
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS user_discord_accounts_discord_user_id_idx
    ON user_discord_accounts (discord_user_id);
