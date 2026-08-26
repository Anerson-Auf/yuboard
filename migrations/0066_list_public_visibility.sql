-- A public board may contain whole internal columns that guests must not see.
-- Signed-in users keep their normal board access; this setting only controls
-- the anonymous public view, just like `cards.is_public`.
ALTER TABLE lists
    ADD COLUMN IF NOT EXISTS is_public BOOLEAN NOT NULL DEFAULT TRUE;
