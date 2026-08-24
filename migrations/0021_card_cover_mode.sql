ALTER TABLE cards
    ADD COLUMN cover_mode TEXT NOT NULL DEFAULT 'full'
    CHECK (cover_mode IN ('full', 'top'));
