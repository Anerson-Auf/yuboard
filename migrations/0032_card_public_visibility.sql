-- A public board may contain cards that are intentionally withheld from guests.
-- Signed-in users can still read them; editing permissions remain unchanged.
ALTER TABLE cards
    ADD COLUMN IF NOT EXISTS is_public BOOLEAN NOT NULL DEFAULT TRUE;
