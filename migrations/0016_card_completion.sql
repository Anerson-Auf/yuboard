ALTER TABLE cards
    ADD COLUMN completed_at TIMESTAMPTZ;

CREATE INDEX cards_board_completion_idx ON cards (board_id, completed_at) WHERE archived_at IS NULL;
