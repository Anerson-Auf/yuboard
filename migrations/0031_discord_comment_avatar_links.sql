-- An opaque per-integration capability lets Discord fetch the avatar of a
-- Flowboard account referenced in a comment without exposing generic avatars.
CREATE TABLE discord_public_media_tokens (
    integration_id UUID PRIMARY KEY REFERENCES discord_integrations(id) ON DELETE CASCADE,
    token UUID NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
