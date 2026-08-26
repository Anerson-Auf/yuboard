-- A zero limit means the list is intentionally unlimited.
ALTER TABLE lists
    ADD COLUMN card_limit INTEGER NOT NULL DEFAULT 0
    CHECK (card_limit >= 0 AND card_limit <= 10000);
