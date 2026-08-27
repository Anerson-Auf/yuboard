-- Lets cursor-based archive pages seek directly to the newest archived cards
-- for one board instead of scanning active or older card history.
CREATE INDEX IF NOT EXISTS cards_board_archived_at_idx
    ON cards (board_id, archived_at DESC, id DESC)
    WHERE archived_at IS NOT NULL;
