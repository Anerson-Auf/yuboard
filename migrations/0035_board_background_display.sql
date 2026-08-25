ALTER TABLE boards
    ADD COLUMN background_fit TEXT NOT NULL DEFAULT 'cover'
        CHECK (background_fit IN ('cover', 'contain', 'fill')),
    ADD COLUMN background_position TEXT NOT NULL DEFAULT 'center'
        CHECK (background_position IN ('top', 'center', 'bottom'));
