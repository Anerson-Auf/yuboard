-- A card may be visually detached in the shared freeform view while keeping
-- its canonical list_id and ordering for the normal Kanban view.
CREATE TABLE board_freeform_card_positions (
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    x INTEGER NOT NULL CHECK (x >= 0 AND x <= 200000),
    y INTEGER NOT NULL CHECK (y >= 0 AND y <= 200000),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (board_id, card_id)
);

CREATE INDEX board_freeform_card_positions_board_idx
    ON board_freeform_card_positions (board_id, updated_at DESC);
