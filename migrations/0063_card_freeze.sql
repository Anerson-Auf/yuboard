ALTER TABLE cards
    ADD COLUMN is_frozen BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX cards_board_frozen_idx
    ON cards (board_id, is_frozen)
    WHERE archived_at IS NULL AND is_frozen;
